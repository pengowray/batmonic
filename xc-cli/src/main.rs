use clap::{Parser, Subcommand};
use std::path::PathBuf;
use xc_lib::{api, cache, key_store, taxonomy, XC_GROUPS};

#[derive(Parser)]
#[command(name = "xc-fetch", about = "Fetch recordings from xeno-canto API v3")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch a single recording by XC number
    Fetch {
        /// Xeno-canto catalogue number (e.g. 928094, XC928094, or URL)
        recording: String,

        /// Fetch metadata only (skip audio download)
        #[arg(long)]
        metadata_only: bool,

        /// Output/cache directory (default: current directory)
        #[arg(long)]
        cache_dir: Option<PathBuf>,

        /// API key (overrides stored key and XC_API_KEY env var)
        #[arg(long)]
        key: Option<String>,
    },
    /// Browse species for a group
    Browse {
        /// Group name: bats, birds, frogs, grasshoppers, "land mammals"
        group: String,

        /// Filter by country
        #[arg(long)]
        country: Option<String>,

        /// API key (overrides stored key and XC_API_KEY env var)
        #[arg(long)]
        key: Option<String>,

        /// Force refresh (ignore cache)
        #[arg(long)]
        refresh: bool,

        /// Cache directory for taxonomy data (default: current directory)
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
    /// Batch download best-quality recordings for each bat species
    BatchBats {
        /// Number of recordings to download per species (default: 2)
        #[arg(long, default_value_t = 2)]
        per_species: u32,

        /// Output/cache directory (default: current directory)
        #[arg(long)]
        cache_dir: Option<PathBuf>,

        /// API key (overrides stored key and XC_API_KEY env var)
        #[arg(long)]
        key: Option<String>,

        /// Delay between downloads in seconds (default: 3)
        #[arg(long, default_value_t = 3)]
        delay: u64,

        /// Skip species that already have enough cached recordings
        #[arg(long, default_value_t = true)]
        skip_cached: bool,

        /// Dry run — show what would be downloaded without actually downloading
        #[arg(long)]
        dry_run: bool,
    },
    /// Delete a cached recording by XC number or filename
    Delete {
        /// XC number (e.g. 928094, XC928094) or filename
        recording: String,

        /// Cache directory (default: current directory)
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
    /// Save your XC API key (shared with the Oversample desktop app)
    SetKey {
        /// The API key to store
        key: String,
    },
    /// Show the stored API key location and status
    ShowKey,
    /// Remove the stored API key
    ClearKey,
    /// Migrate .xc.json files to new format (hashes nested under "_app")
    Migrate {
        /// Directory containing sounds/ with .xc.json files (default: current directory)
        #[arg(long)]
        cache_dir: Option<PathBuf>,

        /// Dry run — show what would change without modifying files
        #[arg(long)]
        dry_run: bool,
    },
    /// Re-fetch metadata from the XC API and update .xc.json sidecar files
    RefreshMetadata {
        /// Directory containing sounds/ with .xc.json files (default: current directory)
        #[arg(long)]
        cache_dir: Option<PathBuf>,

        /// API key (overrides stored key and XC_API_KEY env var)
        #[arg(long)]
        key: Option<String>,

        /// Dry run — show what would change without modifying files
        #[arg(long)]
        dry_run: bool,

        /// Delay between API requests in seconds (default: 1)
        #[arg(long, default_value_t = 1)]
        delay: u64,
    },
    /// Recompute and add missing hashes (spot_hash_b3, content_hash, etc.) to .xc.json files
    Rehash {
        /// Directory containing sounds/ with .xc.json and audio files (default: current directory)
        #[arg(long)]
        cache_dir: Option<PathBuf>,

        /// Dry run — show what would change without modifying files
        #[arg(long)]
        dry_run: bool,

        /// Force recompute all hashes even if already present
        #[arg(long)]
        force: bool,
    },
}

fn require_api_key(explicit: &Option<String>) -> String {
    key_store::resolve_key(explicit).unwrap_or_else(|| {
        eprintln!("API key required. Options:");
        eprintln!("  xc-fetch set-key YOUR_KEY   (saves for reuse)");
        eprintln!("  --key YOUR_KEY              (one-time use)");
        eprintln!("  XC_API_KEY env var or .env   (environment)");
        std::process::exit(1);
    })
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    let client = reqwest::Client::new();

    match cli.command {
        Commands::SetKey { key } => match key_store::save_key(&key) {
            Ok(path) => println!("API key saved to {}", path.display()),
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        },

        Commands::ShowKey => {
            match key_store::key_path() {
                Some(path) => {
                    println!("Key file: {}", path.display());
                    match key_store::load_key() {
                        Some(k) => {
                            let masked = if k.len() > 8 {
                                format!("{}...{}", &k[..4], &k[k.len() - 4..])
                            } else {
                                "****".to_string()
                            };
                            println!("Status:   set ({})", masked);
                        }
                        None => println!("Status:   not set"),
                    }
                }
                None => println!("Could not determine config directory"),
            }
            // Also check env
            if let Ok(k) = std::env::var("XC_API_KEY") {
                if !k.is_empty() {
                    println!("Env var:  XC_API_KEY is set");
                }
            }
        }

        Commands::ClearKey => match key_store::delete_key() {
            Ok(()) => println!("API key removed"),
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        },

        Commands::Fetch {
            recording,
            metadata_only,
            cache_dir,
            key,
        } => {
            let api_key = require_api_key(&key);
            let xc_number = api::parse_xc_number(&recording).unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(1);
            });

            let cache_root = cache_dir.unwrap_or_else(|| PathBuf::from("."));

            eprintln!("Fetching XC{xc_number}...");

            let rec = api::fetch_recording(&client, &api_key, xc_number)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                });

            if metadata_only {
                let sounds_dir = cache_root.join("sounds");
                std::fs::create_dir_all(&sounds_dir).expect("Failed to create sounds dir");
                let stem = cache::recording_stem(&rec);
                let meta_filename = format!("{stem}.xc.json");
                let meta_path = sounds_dir.join(&meta_filename);
                let metadata = cache::build_metadata_json(&rec);
                let json_str = serde_json::to_string_pretty(&metadata).unwrap();
                std::fs::write(&meta_path, format!("{json_str}\n")).unwrap();
                eprintln!("Wrote {}", meta_path.display());
            } else {
                eprintln!("Downloading audio...");
                let audio_bytes = api::download_audio(&client, &rec.file_url)
                    .await
                    .unwrap_or_else(|e| {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    });

                let audio_path = cache::save_recording(&cache_root, &rec, &audio_bytes, None)
                    .unwrap_or_else(|e| {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    });

                eprintln!(
                    "Wrote {} ({:.1} MB)",
                    audio_path.display(),
                    audio_bytes.len() as f64 / 1_048_576.0
                );
            }

            println!("XC{}: {} ({} {})", rec.id, rec.en, rec.genus, rec.sp);
            println!("Recordist: {}", rec.rec);
            println!("License: {}", rec.lic);
            println!(
                "Attribution: {}, XC{}. Accessible at www.xeno-canto.org/{}",
                rec.rec, rec.id, rec.id
            );
        }

        Commands::Browse {
            group,
            country,
            key,
            refresh,
            cache_dir,
        } => {
            if !XC_GROUPS.contains(&group.as_str()) {
                eprintln!(
                    "Unknown group '{group}'. Available: {}",
                    XC_GROUPS.join(", ")
                );
                std::process::exit(1);
            }

            let api_key = require_api_key(&key);
            let cache_root = cache_dir.unwrap_or_else(|| PathBuf::from("."));
            let country_ref = country.as_deref();

            if !refresh {
                if let Ok(Some(cached)) = cache::load_taxonomy(&cache_root, &group, country_ref) {
                    let age = cache::taxonomy_age_string(&cache_root, &group, country_ref)
                        .unwrap_or_default();
                    eprintln!("Using cached taxonomy ({})", age);
                    print_taxonomy(&cached);
                    return;
                }
            }

            eprintln!("Fetching species list for '{group}'...");

            let taxonomy = taxonomy::build_species_list(
                &client,
                &api_key,
                &group,
                country_ref,
                |page, total| {
                    eprint!("\rPage {page}/{total}...");
                },
            )
            .await
            .unwrap_or_else(|e| {
                eprintln!("\nError: {e}");
                std::process::exit(1);
            });
            eprintln!();

            if let Err(e) = cache::save_taxonomy(&cache_root, &group, country_ref, &taxonomy) {
                eprintln!("Warning: failed to cache taxonomy: {e}");
            }

            print_taxonomy(&taxonomy);
        }

        Commands::BatchBats {
            per_species,
            cache_dir,
            key,
            delay,
            skip_cached,
            dry_run,
        } => {
            let api_key = require_api_key(&key);
            let cache_root = cache_dir.unwrap_or_else(|| PathBuf::from("."));

            // Step 1: Get bat taxonomy (use cache if available)
            let taxonomy = match cache::load_taxonomy(&cache_root, "bats", None) {
                Ok(Some(cached)) => {
                    let age =
                        cache::taxonomy_age_string(&cache_root, "bats", None).unwrap_or_default();
                    eprintln!("Using cached bat taxonomy ({age})");
                    cached
                }
                _ => {
                    eprintln!("Fetching bat species list...");
                    let tax = taxonomy::build_species_list(
                        &client,
                        &api_key,
                        "bats",
                        None,
                        |page, total| {
                            eprint!("\rPage {page}/{total}...");
                        },
                    )
                    .await
                    .unwrap_or_else(|e| {
                        eprintln!("\nError: {e}");
                        std::process::exit(1);
                    });
                    eprintln!();
                    let _ = cache::save_taxonomy(&cache_root, "bats", None, &tax);
                    tax
                }
            };

            eprintln!(
                "{} bat species, {} total recordings",
                taxonomy.species.len(),
                taxonomy.total_recordings
            );

            // Step 2: For each species, find and download best recordings
            let mut total_downloaded = 0u32;
            let mut total_skipped = 0u32;
            let mut total_errors = 0u32;

            for (sp_idx, species) in taxonomy.species.iter().enumerate() {
                let species_name = format!("{} {} ({})", species.genus, species.sp, species.en);
                eprint!(
                    "\r[{}/{}] {}",
                    sp_idx + 1,
                    taxonomy.species.len(),
                    species_name,
                );

                if species.recording_count == 0 {
                    eprintln!(" — no recordings");
                    continue;
                }

                // Check how many we already have cached for this species
                let existing = if skip_cached {
                    count_cached_for_species(&cache_root, &species.genus, &species.sp)
                } else {
                    0
                };

                if existing >= per_species {
                    total_skipped += 1;
                    eprintln!(" — already have {existing} cached, skipping");
                    continue;
                }

                let needed = per_species - existing;

                // Search for best quality recordings of this species
                // Quality sort: q:A first, then by sample rate descending
                let query = format!("grp:bats gen:{} sp:{}", species.genus, species.sp);

                let search_result = match api::search(&client, &api_key, &query, 1, 50).await {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!(" — search error: {e}");
                        total_errors += 1;
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                        continue;
                    }
                };

                // Sort by quality (A > B > C > D > E), then by sample rate descending
                let mut candidates = search_result.recordings;
                candidates.sort_by(|a, b| {
                    let qa = quality_rank(&a.q);
                    let qb = quality_rank(&b.q);
                    qa.cmp(&qb).then_with(|| {
                        let sa = a.smp.parse::<u64>().unwrap_or(0);
                        let sb = b.smp.parse::<u64>().unwrap_or(0);
                        sb.cmp(&sa) // higher sample rate first
                    })
                });

                // Filter out already-cached recordings
                let candidates: Vec<_> = candidates
                    .into_iter()
                    .filter(|r| !cache::is_recording_cached(&cache_root, r.id_num()))
                    .collect();

                if candidates.is_empty() {
                    eprintln!(" — no new candidates");
                    continue;
                }

                let to_fetch: Vec<_> = candidates.into_iter().take(needed as usize).collect();
                eprintln!();

                for rec in &to_fetch {
                    let smp_display = rec.smp.parse::<u64>().unwrap_or(0);
                    eprintln!(
                        "  Downloading XC{}: q={}, {}kHz, {}",
                        rec.id,
                        rec.q,
                        smp_display / 1000,
                        rec.length
                    );

                    if dry_run {
                        total_downloaded += 1;
                        continue;
                    }

                    // Rate-limit between downloads
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;

                    match api::download_audio(&client, &rec.file_url).await {
                        Ok(audio_bytes) => {
                            match cache::save_recording(&cache_root, rec, &audio_bytes, None) {
                                Ok(path) => {
                                    eprintln!(
                                        "    Saved {} ({:.1} MB)",
                                        path.file_name().unwrap_or_default().to_string_lossy(),
                                        audio_bytes.len() as f64 / 1_048_576.0
                                    );
                                    total_downloaded += 1;
                                }
                                Err(e) => {
                                    eprintln!("    Save error: {e}");
                                    total_errors += 1;
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("    Download error: {e}");
                            total_errors += 1;
                        }
                    }
                }
            }

            eprintln!();
            println!(
                "Done. Downloaded: {total_downloaded}, Skipped: {total_skipped}, Errors: {total_errors}"
            );
        }

        Commands::RefreshMetadata {
            cache_dir,
            key,
            dry_run,
            delay,
        } => {
            let api_key = require_api_key(&key);
            let root = cache_dir.unwrap_or_else(|| PathBuf::from("."));
            let sounds_dir = root.join("sounds");
            if !sounds_dir.exists() {
                eprintln!("No sounds/ directory found at {}", root.display());
                std::process::exit(1);
            }

            // Find all .xc.json files and extract XC IDs
            let json_entries: Vec<_> = std::fs::read_dir(&sounds_dir)
                .unwrap_or_else(|e| {
                    eprintln!("Error reading {}: {e}", sounds_dir.display());
                    std::process::exit(1);
                })
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".xc.json"))
                .collect();

            eprintln!("Found {} .xc.json files", json_entries.len());

            let mut updated = 0u32;
            let mut skipped = 0u32;
            let mut errors = 0u32;

            for entry in &json_entries {
                let json_path = entry.path();
                let json_name = entry.file_name().to_string_lossy().to_string();

                // Extract XC ID from filename (e.g. "XC928094 - ...")
                let xc_id: u64 = json_name
                    .strip_prefix("XC")
                    .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                if xc_id == 0 {
                    eprintln!("  Skipping {json_name}: can't parse XC ID");
                    skipped += 1;
                    continue;
                }

                // Read existing sidecar to preserve _app hashes
                let content = match std::fs::read_to_string(&json_path) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("  Error reading {json_name}: {e}");
                        errors += 1;
                        continue;
                    }
                };
                let existing_json: serde_json::Value = match serde_json::from_str(&content) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("  Error parsing {json_name}: {e}");
                        errors += 1;
                        continue;
                    }
                };

                // Rate-limit
                if updated > 0 || errors > 0 {
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                }

                // Fetch fresh metadata from API
                eprint!("  Fetching XC{xc_id}...");
                let rec = match api::fetch_recording(&client, &api_key, xc_id).await {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!(" error: {e}");
                        errors += 1;
                        continue;
                    }
                };

                // Build new metadata JSON
                let mut new_json = cache::build_metadata_json(&rec);

                // Preserve existing _app section (hashes, retrieved date, etc.)
                if let Some(app) = existing_json.get("_app") {
                    new_json
                        .as_object_mut()
                        .unwrap()
                        .insert("_app".into(), app.clone());
                }

                if dry_run {
                    eprintln!(" would update");
                } else {
                    let json_str = serde_json::to_string_pretty(&new_json).unwrap();
                    if let Err(e) = std::fs::write(&json_path, format!("{json_str}\n")) {
                        eprintln!(" write error: {e}");
                        errors += 1;
                        continue;
                    }
                    eprintln!(" updated");
                }
                updated += 1;
            }

            println!(
                "Done. Updated: {updated}, Skipped: {skipped}, Errors: {errors}{}",
                if dry_run { " (dry run)" } else { "" }
            );
        }

        Commands::Migrate { cache_dir, dry_run } => {
            let root = cache_dir.unwrap_or_else(|| PathBuf::from("."));
            let sounds_dir = root.join("sounds");
            if !sounds_dir.exists() {
                eprintln!("No sounds/ directory found at {}", root.display());
                std::process::exit(1);
            }

            let mut migrated = 0u32;
            let mut skipped = 0u32;
            let mut errors = 0u32;

            let entries: Vec<_> = std::fs::read_dir(&sounds_dir)
                .unwrap_or_else(|e| {
                    eprintln!("Error reading {}: {e}", sounds_dir.display());
                    std::process::exit(1);
                })
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".xc.json"))
                .collect();

            eprintln!("Found {} .xc.json files", entries.len());

            for entry in &entries {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();

                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("  Error reading {name}: {e}");
                        errors += 1;
                        continue;
                    }
                };

                let mut json: serde_json::Value = match serde_json::from_str(&content) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("  Error parsing {name}: {e}");
                        errors += 1;
                        continue;
                    }
                };

                if cache::migrate_sidecar_json(&mut json) {
                    if dry_run {
                        eprintln!("  Would migrate: {name}");
                    } else {
                        let json_str = serde_json::to_string_pretty(&json).unwrap();
                        if let Err(e) = std::fs::write(&path, format!("{json_str}\n")) {
                            eprintln!("  Error writing {name}: {e}");
                            errors += 1;
                            continue;
                        }
                        eprintln!("  Migrated: {name}");
                    }
                    migrated += 1;
                } else {
                    skipped += 1;
                }
            }

            println!(
                "Done. Migrated: {migrated}, Already current: {skipped}, Errors: {errors}{}",
                if dry_run { " (dry run)" } else { "" }
            );
        }

        Commands::Rehash {
            cache_dir,
            dry_run,
            force,
        } => {
            let root = cache_dir.unwrap_or_else(|| PathBuf::from("."));
            let sounds_dir = root.join("sounds");
            if !sounds_dir.exists() {
                eprintln!("No sounds/ directory found at {}", root.display());
                std::process::exit(1);
            }

            // Find all .xc.json files
            let json_entries: Vec<_> = std::fs::read_dir(&sounds_dir)
                .unwrap_or_else(|e| {
                    eprintln!("Error reading {}: {e}", sounds_dir.display());
                    std::process::exit(1);
                })
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".xc.json"))
                .collect();

            eprintln!("Found {} .xc.json files", json_entries.len());

            let mut updated = 0u32;
            let mut skipped = 0u32;
            let mut errors = 0u32;

            for entry in &json_entries {
                let json_path = entry.path();
                let json_name = entry.file_name().to_string_lossy().to_string();

                // Find corresponding audio file
                let stem = json_name.strip_suffix(".xc.json").unwrap();
                let audio_path = find_audio_file(&sounds_dir, stem);
                let Some(audio_path) = audio_path else {
                    eprintln!("  No audio file found for {json_name}");
                    errors += 1;
                    continue;
                };

                // Read and parse JSON
                let content = match std::fs::read_to_string(&json_path) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("  Error reading {json_name}: {e}");
                        errors += 1;
                        continue;
                    }
                };
                let mut json: serde_json::Value = match serde_json::from_str(&content) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("  Error parsing {json_name}: {e}");
                        errors += 1;
                        continue;
                    }
                };

                // Check what hashes are missing
                let app = if json["_app"].is_object() {
                    &json["_app"]
                } else {
                    &json
                };
                let has_spot_b3 = app["spot_hash_b3"].is_string();
                let has_content = app["content_hash"].is_string();
                let has_blake3 = app["blake3"].is_string();
                let has_sha256 = app["sha256"].is_string();

                if !force && has_spot_b3 && has_content && has_blake3 && has_sha256 {
                    skipped += 1;
                    continue;
                }

                // Read audio file
                let audio_bytes = match std::fs::read(&audio_path) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("  Error reading audio {}: {e}", audio_path.display());
                        errors += 1;
                        continue;
                    }
                };

                let hashes = cache::compute_file_hashes(&audio_bytes);

                // Ensure _app object exists
                if !json["_app"].is_object() {
                    // Migrate first
                    cache::migrate_sidecar_json(&mut json);
                }
                if !json["_app"].is_object() {
                    json.as_object_mut()
                        .unwrap()
                        .insert("_app".into(), serde_json::json!({}));
                }
                let app_obj = json["_app"].as_object_mut().unwrap();

                let mut changes = Vec::new();
                if force || !has_blake3 {
                    app_obj.insert("blake3".into(), serde_json::json!(hashes.blake3));
                    changes.push("blake3");
                }
                if force || !has_sha256 {
                    app_obj.insert("sha256".into(), serde_json::json!(hashes.sha256));
                    changes.push("sha256");
                }
                if force || !has_spot_b3 {
                    app_obj.insert(
                        "spot_hash_b3".into(),
                        serde_json::json!(hashes.spot_hash_b3),
                    );
                    changes.push("spot_hash_b3");
                }
                if force || !has_content {
                    app_obj.insert(
                        "content_hash".into(),
                        serde_json::json!(hashes.content_hash),
                    );
                    changes.push("content_hash");
                }
                // Always ensure file_size, data_offset, data_size are present
                app_obj.insert("file_size".into(), serde_json::json!(hashes.size_bytes));
                if let Some(offset) = hashes.data_offset {
                    app_obj.insert("data_offset".into(), serde_json::json!(offset));
                }
                if let Some(size) = hashes.data_size {
                    app_obj.insert("data_size".into(), serde_json::json!(size));
                }
                // Remove old spot_hash if present
                app_obj.remove("spot_hash");

                if changes.is_empty() && !force {
                    skipped += 1;
                    continue;
                }

                if dry_run {
                    eprintln!("  Would update {json_name}: +{}", changes.join(", "));
                } else {
                    let json_str = serde_json::to_string_pretty(&json).unwrap();
                    if let Err(e) = std::fs::write(&json_path, format!("{json_str}\n")) {
                        eprintln!("  Error writing {json_name}: {e}");
                        errors += 1;
                        continue;
                    }
                    eprintln!("  Updated {json_name}: +{}", changes.join(", "));
                }
                updated += 1;
            }

            println!(
                "Done. Updated: {updated}, Already complete: {skipped}, Errors: {errors}{}",
                if dry_run { " (dry run)" } else { "" }
            );
        }

        Commands::Delete {
            recording,
            cache_dir,
        } => {
            let cache_root = cache_dir.unwrap_or_else(|| PathBuf::from("."));

            // Try to parse as XC number first
            let id = match api::parse_xc_number(&recording) {
                Ok(n) => n,
                Err(_) => {
                    // Try to extract XC number from filename (e.g. "XC928094 - ...")
                    if let Some(rest) = recording.strip_prefix("XC") {
                        rest.split(|c: char| !c.is_ascii_digit())
                            .next()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or_else(|| {
                                eprintln!("Can't parse XC number from: {recording}");
                                std::process::exit(1);
                            })
                    } else {
                        eprintln!("Can't parse XC number from: {recording}");
                        std::process::exit(1);
                    }
                }
            };

            match cache::delete_recording(&cache_root, id) {
                Ok(deleted) => {
                    for name in &deleted {
                        println!("Deleted: {name}");
                    }
                    println!("Removed {} file(s) for XC{id}", deleted.len());
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Rank quality ratings: A=0 (best), B=1, C=2, D=3, E=4, unknown=5
fn quality_rank(q: &str) -> u8 {
    match q.trim() {
        "A" => 0,
        "B" => 1,
        "C" => 2,
        "D" => 3,
        "E" => 4,
        _ => 5,
    }
}

/// Count how many recordings for a species are already cached.
fn count_cached_for_species(root: &std::path::Path, genus: &str, sp: &str) -> u32 {
    let sounds_dir = root.join("sounds");
    if !sounds_dir.exists() {
        return 0;
    }
    let suffix = format!(" - {} {}", genus, sp);
    let mut count = 0u32;
    if let Ok(entries) = std::fs::read_dir(&sounds_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("XC") && !name.ends_with(".xc.json") {
                let without_ext = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name);
                if without_ext.ends_with(&suffix) {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Find the audio file corresponding to a .xc.json stem in a sounds directory.
/// Scans the directory for any file matching the stem with a non-.xc.json extension.
fn find_audio_file(sounds_dir: &std::path::Path, stem: &str) -> Option<PathBuf> {
    if let Ok(entries) = std::fs::read_dir(sounds_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(without_ext) = name.rsplit_once('.').map(|(s, _)| s) {
                if without_ext == stem && !name.ends_with(".xc.json") {
                    return Some(entry.path());
                }
            }
        }
    }
    None
}

fn print_taxonomy(taxonomy: &xc_lib::XcGroupTaxonomy) {
    println!(
        "{} species, {} recordings ({})",
        taxonomy.species.len(),
        taxonomy.total_recordings,
        taxonomy.group
    );
    println!();
    for sp in &taxonomy.species {
        println!(
            "  {:40} {:30} {:>5} recordings",
            sp.en,
            format!("{} {}", sp.genus, sp.sp),
            sp.recording_count
        );
    }
}
