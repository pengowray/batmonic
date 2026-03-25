use super::country_map::country_to_region;
use super::data::get_manifest;
use super::types::{AutoResolved, BatBookRegion};
use crate::state::LoadedFile;

/// Search all regional bat books for a species by scientific name.
/// Returns `(region, species_id)` for the best match.
/// Prefers `preferred_region` if the species appears there; otherwise picks the
/// first non-Global match.
fn find_species_across_books(
    scientific_name: &str,
    preferred_region: Option<BatBookRegion>,
) -> Option<(BatBookRegion, String)> {
    let query = scientific_name.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }

    // Also try binomial (first two words) for trinomials like "Myotis lucifugus alascensis"
    let binomial: Option<String> = {
        let parts: Vec<&str> = query.split_whitespace().collect();
        if parts.len() > 2 {
            Some(format!("{} {}", parts[0], parts[1]))
        } else {
            None
        }
    };

    let mut best: Option<(BatBookRegion, String)> = None;

    for &region in BatBookRegion::ALL {
        if region == BatBookRegion::Global {
            continue; // Global only has family-level entries, skip
        }
        let manifest = get_manifest(region);
        for entry in &manifest.entries {
            let sci = entry.scientific_name.to_lowercase();
            if sci == query || binomial.as_deref() == Some(&sci) {
                // Exact match in this region
                if preferred_region == Some(region) {
                    // Best possible: species is in the country's own region
                    return Some((region, entry.id.to_string()));
                }
                if best.is_none() {
                    best = Some((region, entry.id.to_string()));
                }
            }
        }
    }

    best
}

/// Extract species scientific name from file metadata.
/// Tries XC "Scientific name" first, then GUANO Species|Manual, then Species|Auto.
fn get_scientific_name(file: &LoadedFile) -> Option<String> {
    // XC metadata
    if let Some(meta) = &file.xc_metadata {
        if let Some(val) = meta.iter().find(|(k, _)| k == "Scientific name").map(|(_, v)| v.clone()) {
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    // GUANO fields
    if let Some(guano) = &file.audio.metadata.guano {
        // Prefer manual over auto
        for key in &["Species|Manual", "Species|Auto"] {
            if let Some(val) = guano.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()) {
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
    }
    None
}

/// Extract country from XC metadata.
fn get_country(file: &LoadedFile) -> Option<String> {
    file.xc_metadata.as_ref()?
        .iter()
        .find(|(k, _)| k == "Country")
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
}

/// Resolve the bat book automatically from the current file's metadata.
///
/// Priority:
/// 1. Species + country → use country's region, highlight matched species
/// 2. Species only → use region where species was found
/// 3. Country only → use country's region
/// 4. No metadata → first favourite, then Global
pub fn resolve_auto(file: Option<&LoadedFile>, favourites: &[BatBookRegion]) -> AutoResolved {
    let fallback_region = favourites.first().copied().unwrap_or(BatBookRegion::Global);

    let Some(file) = file else {
        return AutoResolved {
            region: fallback_region,
            matched_species_id: None,
            source_label: fallback_region.short_label().to_string(),
        };
    };

    let scientific_name = get_scientific_name(file);
    let country = get_country(file);
    let country_region = country.as_deref().and_then(country_to_region);

    // Try species lookup
    if let Some(ref sci) = scientific_name {
        if let Some((region, species_id)) = find_species_across_books(sci, country_region) {
            // Use the country's region if available, otherwise use the region where species was found
            let effective_region = country_region.unwrap_or(region);
            // But verify the species exists in the effective region too
            let (final_region, final_id) = if effective_region != region {
                // Check if species is also in the country's region
                if let Some((_, id)) = find_species_across_books(sci, Some(effective_region)) {
                    (effective_region, id)
                } else {
                    // Species not in the country's region — show country's book anyway,
                    // but still highlight the species from whichever book had it
                    (effective_region, species_id)
                }
            } else {
                (region, species_id)
            };
            let label = if let Some(ref cnt) = country {
                format!("{} ({})", final_region.short_label(), cnt)
            } else {
                final_region.short_label().to_string()
            };
            return AutoResolved {
                region: final_region,
                matched_species_id: Some(final_id),
                source_label: label,
            };
        }
    }

    // No species match — try country only
    if let Some(region) = country_region {
        let label = if let Some(ref cnt) = country {
            format!("{} ({})", region.short_label(), cnt)
        } else {
            region.short_label().to_string()
        };
        return AutoResolved {
            region,
            matched_species_id: None,
            source_label: label,
        };
    }

    // Nothing useful — fallback
    AutoResolved {
        region: fallback_region,
        matched_species_id: None,
        source_label: fallback_region.short_label().to_string(),
    }
}

/// Find the BatBookEntry for a species id in a given region's manifest.
/// Used by the strip to render the auto-matched species chip.
pub fn find_entry_in_manifest(
    region: BatBookRegion,
    species_id: &str,
) -> Option<super::types::BatBookEntry> {
    let manifest = get_manifest(region);
    manifest.entries.into_iter().find(|e| e.id == species_id)
}

/// Try to find the entry across ALL books (for when the matched species
/// came from a different region than the displayed one).
pub fn find_entry_any_book(species_id: &str) -> Option<super::types::BatBookEntry> {
    for &region in BatBookRegion::ALL {
        let manifest = get_manifest(region);
        if let Some(entry) = manifest.entries.into_iter().find(|e| e.id == species_id) {
            return Some(entry);
        }
    }
    None
}
