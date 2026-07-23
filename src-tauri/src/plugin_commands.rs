// SPDX-License-Identifier: GPL-3.0-only
// Single source of truth for the inlined Android plugin commands.
//
// THREE-WAY COUPLING — these must stay in sync:
//   1. PLUGIN_COMMANDS (below) — `build.rs` loops over this to register each
//      plugin's command permissions, and the tests below check it.
//   2. `capabilities/default.json` — the runtime allowlist: one
//      `"<plugin>:allow-<command>"` entry per command (plus `core:default`).
//   3. the Kotlin `@Command fun <name>` in each `*Plugin.kt` under
//      `gen/android/app/src/main/java/com/oversample/app/`.
//
// A mismatch between (1) and (2) fails `default_capabilities_match_registered_commands`.
// A registered command with no Kotlin `@Command` fails `registered_commands_have_kotlin_impl`.
// The Kotlin files may legitimately define EXTRA @Commands that aren't exposed as
// permissions (e.g. usb-audio's requestAudioPermission/checkUsbStatus, media-store's
// cleanupPendingEntries), so that direction is intentionally not enforced.
//
// Without this guard a typo or renamed command surfaces only as a runtime IPC
// rejection on-device, never as a build/test failure.

/// `(plugin_name, &[command_name, ...])` for every inlined Tauri plugin.
/// Command names are camelCase and match the Kotlin `@Command fun` names; the
/// generated permission identifier for each is `"<plugin>:allow-<command>"`.
pub const PLUGIN_COMMANDS: &[(&str, &[&str])] = &[
    (
        "usb-audio",
        &[
            "listUsbDevices",
            "requestUsbPermission",
            "getUsbDeviceInfo",
            "openUsbDevice",
            "closeUsbDevice",
        ],
    ),
    (
        "media-store",
        &[
            "saveToSharedStorage",
            "saveWavBytes",
            "saveExportBytes",
            "createRecordingEntry",
            "finalizeRecordingEntry",
            "cancelRecordingEntry",
            "exportFile",
            "readRecordingRange",
        ],
    ),
    (
        "geolocation",
        &["getCurrentLocation", "getWifiSsid", "getDeviceModel"],
    ),
    ("zoom", &["reset"]),
    (
        "audio-service",
        &[
            "startForegroundAudio",
            "updateForegroundAudio",
            "stopForegroundAudio",
            "isIgnoringBatteryOptimizations",
            "requestDisableBatteryOptimization",
            "isNotificationPermissionGranted",
            "requestNotificationPermission",
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::PLUGIN_COMMANDS;
    use std::collections::HashSet;

    /// Every `<plugin>:allow-<command>` in default.json must reference a
    /// registered command, and every registered command must be granted —
    /// catches typos, renamed/removed commands, and forgotten allowlist entries.
    #[test]
    fn default_capabilities_match_registered_commands() {
        let json = include_str!("../capabilities/default.json");
        let parsed: serde_json::Value =
            serde_json::from_str(json).expect("capabilities/default.json should be valid JSON");
        let perms = parsed["permissions"]
            .as_array()
            .expect("default.json should have a permissions array");

        let plugins: HashSet<&str> = PLUGIN_COMMANDS.iter().map(|(p, _)| *p).collect();

        // Expected `<plugin>:allow-<command>` identifiers from the registry.
        let expected: HashSet<String> = PLUGIN_COMMANDS
            .iter()
            .flat_map(|(plugin, cmds)| cmds.iter().map(move |c| format!("{plugin}:allow-{c}")))
            .collect();

        // Plugin permissions actually listed (ignore core:* and any non-plugin entry).
        let listed: HashSet<String> = perms
            .iter()
            .map(|p| {
                p.as_str()
                    .expect("permission entries are strings")
                    .to_string()
            })
            .filter(|s| {
                s.split(':')
                    .next()
                    .map(|p| plugins.contains(p))
                    .unwrap_or(false)
            })
            .collect();

        let unknown: Vec<&String> = listed.difference(&expected).collect();
        assert!(
            unknown.is_empty(),
            "default.json grants permissions with no matching registered command \
             (typo or stale entry): {unknown:?}"
        );

        let ungranted: Vec<&String> = expected.difference(&listed).collect();
        assert!(
            ungranted.is_empty(),
            "registered commands missing an allow- permission in default.json: {ungranted:?}"
        );
    }

    /// Collect every Kotlin `@Command fun <name>` under the android plugin dir.
    /// Returns None if the generated android project isn't present, so the test
    /// is a no-op on checkouts that haven't run `tauri android init`.
    fn kotlin_command_names() -> Option<HashSet<String>> {
        let dir = std::path::Path::new("gen/android/app/src/main/java/com/oversample/app");
        let entries = std::fs::read_dir(dir).ok()?;
        let mut names = HashSet::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("kt") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            // For each `@Command`, grab the identifier of the next `fun`.
            for (idx, _) in content.match_indices("@Command") {
                let rest = &content[idx..];
                if let Some(fun_off) = rest.find("fun ") {
                    let name: String = rest[fun_off + 4..]
                        .chars()
                        .skip_while(|c| c.is_whitespace())
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        names.insert(name);
                    }
                }
            }
        }
        Some(names)
    }

    /// Every registered command must have a Kotlin `@Command fun` of the same
    /// name — catches a renamed/typo'd Kotlin command that would otherwise only
    /// fail at runtime on-device. (Kotlin may define extra @Commands; allowed.)
    #[test]
    fn registered_commands_have_kotlin_impl() {
        let Some(kotlin) = kotlin_command_names() else {
            eprintln!("skipping: gen/android plugin sources not present");
            return;
        };
        let missing: Vec<String> = PLUGIN_COMMANDS
            .iter()
            .flat_map(|(plugin, cmds)| {
                cmds.iter()
                    .filter(|c| !kotlin.contains(**c))
                    .map(move |c| format!("{plugin}:{c}"))
            })
            .collect();
        assert!(
            missing.is_empty(),
            "registered commands with no matching Kotlin @Command fun \
             (renamed or typo?): {missing:?}"
        );
    }
}
