use crate::api;
use crate::types::{XcGroupTaxonomy, XcSpecies};
use std::collections::HashMap;

/// Build a species list for a group by paginating through all API results.
///
/// The `on_progress` callback receives `(pages_fetched, total_pages)`.
pub async fn build_species_list<F>(
    client: &reqwest::Client,
    api_key: &str,
    group: &str,
    country: Option<&str>,
    mut on_progress: F,
) -> Result<XcGroupTaxonomy, String>
where
    F: FnMut(u32, u32),
{
    let mut query = format!("grp:{group}");
    if let Some(cnt) = country {
        query.push_str(&format!(" cnt:\"{cnt}\""));
    }

    // Species key -> (en, count)
    let mut species_map: HashMap<(String, String), (String, u32)> = HashMap::new();
    let mut total_recordings;

    let per_page = 500;
    let mut page = 1u32;
    let mut total_pages;

    loop {
        let result = api::search(client, api_key, &query, page, per_page).await?;
        total_pages = result.num_pages;
        total_recordings = result.num_recordings;
        on_progress(page, total_pages);

        for rec in &result.recordings {
            let key = (rec.genus.clone(), rec.sp.clone());
            let entry = species_map
                .entry(key)
                .or_insert_with(|| (rec.en.clone(), 0));
            entry.1 += 1;
        }

        if page >= total_pages {
            break;
        }
        page += 1;
    }

    let mut species: Vec<XcSpecies> = species_map
        .into_iter()
        .map(|((genus, sp), (en, count))| XcSpecies {
            genus,
            sp,
            en,
            fam: String::new(),
            recording_count: count,
        })
        .collect();

    // Sort by English name
    species.sort_by(|a, b| a.en.to_lowercase().cmp(&b.en.to_lowercase()));

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    Ok(XcGroupTaxonomy {
        group: group.to_string(),
        country: country.map(|s| s.to_string()),
        species,
        total_recordings,
        last_updated: now,
    })
}
