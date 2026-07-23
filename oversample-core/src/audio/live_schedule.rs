//! Pure scheduling math for live-audio playback (bounded-lookahead skip-ahead).
//!
//! The live-listening playback path schedules each incoming mic chunk onto a
//! Web Audio `AudioContext` at a running cursor (`next_time`). When the WebView
//! is throttled (app backgrounded) the queued chunks burst-replay and the cursor
//! races ahead of the context clock, producing a multi-second playback backlog
//! that keeps sounding after Stop. This helper bounds that: if the cursor has
//! fallen behind the clock (underrun) or run too far ahead of it (backlog), it
//! snaps the cursor back to "now" — the "skip-ahead, never play behind" policy.

/// Default maximum seconds the schedule cursor may lead the context clock before
/// the lead is treated as a backlog to drop. ~0.3 s = a few 80 ms chunks of
/// headroom: enough to absorb normal jitter, small enough to stay perceptibly
/// live.
pub const DEFAULT_MAX_LOOKAHEAD_SECS: f64 = 0.30;

/// Outcome of [`plan_live_schedule`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScheduleDecision {
    /// AudioContext time at which to start the next buffer.
    pub start: f64,
    /// True when a backlog was dropped (the cursor led the clock by more than
    /// `max_lookahead`). The caller should stop already-scheduled sources (so
    /// the stale tail never sounds) and count a skip event for throttle
    /// detection.
    pub dropped_backlog: bool,
}

/// Decide where to schedule the next live-audio chunk.
///
/// * `current_time` — the AudioContext's current clock time.
/// * `next_time` — the running schedule cursor (end of the last scheduled buffer).
/// * `max_lookahead` — max seconds `next_time` may lead `current_time` before the
///   lead is treated as a backlog (see [`DEFAULT_MAX_LOOKAHEAD_SECS`]).
pub fn plan_live_schedule(
    current_time: f64,
    next_time: f64,
    max_lookahead: f64,
) -> ScheduleDecision {
    if next_time < current_time {
        // Underrun: the cursor fell behind real time (queue drained or the clock
        // jumped forward after a resume). Resume from now; nothing to drop.
        ScheduleDecision {
            start: current_time,
            dropped_backlog: false,
        }
    } else if next_time - current_time > max_lookahead {
        // Backlog: too much audio queued ahead (burst replay after throttling).
        // Snap to now and tell the caller to drop the stale scheduled tail.
        ScheduleDecision {
            start: current_time,
            dropped_backlog: true,
        }
    } else {
        // Steady state: continue seamlessly from the cursor.
        ScheduleDecision {
            start: next_time,
            dropped_backlog: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX: f64 = DEFAULT_MAX_LOOKAHEAD_SECS;

    #[test]
    fn steady_state_continues_from_cursor() {
        // One 80 ms chunk of lead — well within budget.
        let d = plan_live_schedule(10.0, 10.08, MAX);
        assert_eq!(d.start, 10.08);
        assert!(!d.dropped_backlog);
    }

    #[test]
    fn equal_times_continue_from_now() {
        let d = plan_live_schedule(10.0, 10.0, MAX);
        assert_eq!(d.start, 10.0);
        assert!(!d.dropped_backlog);
    }

    #[test]
    fn underrun_resumes_from_now_without_dropping() {
        // Cursor behind the clock → snap to now, but not a "backlog drop".
        let d = plan_live_schedule(10.0, 9.5, MAX);
        assert_eq!(d.start, 10.0);
        assert!(!d.dropped_backlog);
    }

    #[test]
    fn backlog_is_dropped_and_snaps_to_now() {
        // Cursor 10 s ahead (post-background burst) → drop backlog, snap to now.
        let d = plan_live_schedule(10.0, 20.0, MAX);
        assert_eq!(d.start, 10.0);
        assert!(d.dropped_backlog);
    }

    #[test]
    fn just_under_boundary_is_not_dropped() {
        // A lead comfortably within the budget continues from the cursor.
        let next = 10.0 + MAX - 0.01;
        let d = plan_live_schedule(10.0, next, MAX);
        assert_eq!(d.start, next);
        assert!(!d.dropped_backlog);
    }

    #[test]
    fn just_over_boundary_is_dropped() {
        let d = plan_live_schedule(10.0, 10.0 + MAX + 0.01, MAX);
        assert_eq!(d.start, 10.0);
        assert!(d.dropped_backlog);
    }
}
