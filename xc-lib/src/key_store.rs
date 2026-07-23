use std::path::PathBuf;

/// The app identifier used by Tauri (must match tauri.conf.json).
const APP_IDENTIFIER: &str = "com.oversample.app";
const KEY_FILENAME: &str = "xc_api_key.txt";

/// Resolve the directory where Tauri stores app config.
///
/// On Windows: `%APPDATA%\com.oversample.app\`
/// On macOS:   `~/Library/Application Support/com.oversample.app/`
/// On Linux:   `$XDG_CONFIG_HOME/com.oversample.app/` or `~/.config/com.oversample.app/`
fn app_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .ok()
            .map(|d| PathBuf::from(d).join(APP_IDENTIFIER))
    }
    #[cfg(target_os = "macos")]
    {
        dirs_like_home().map(|h| h.join("Library/Application Support").join(APP_IDENTIFIER))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(dirs_like_home_config)
            .map(|d| d.join(APP_IDENTIFIER))
    }
}

#[cfg(target_os = "macos")]
fn dirs_like_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn dirs_like_home_config() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config"))
}

/// Get the path to the stored API key file (shared with Tauri app).
pub fn key_path() -> Option<PathBuf> {
    app_config_dir().map(|d| d.join(KEY_FILENAME))
}

/// Read the stored API key, if any.
pub fn load_key() -> Option<String> {
    let path = key_path()?;
    let key = std::fs::read_to_string(path).ok()?;
    let key = key.trim().to_string();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

/// Save an API key to the shared config location.
pub fn save_key(key: &str) -> Result<PathBuf, String> {
    let path = key_path().ok_or("Could not determine config directory")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {e}"))?;
    }
    std::fs::write(&path, key.trim()).map_err(|e| format!("Failed to write API key: {e}"))?;
    Ok(path)
}

/// Remove the stored API key.
pub fn delete_key() -> Result<(), String> {
    if let Some(path) = key_path() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("Failed to remove API key: {e}"))?;
        }
    }
    Ok(())
}

/// Resolve an API key from (in priority order):
/// 1. Explicit `--key` argument
/// 2. Stored key (shared with Tauri app)
/// 3. `XC_API_KEY` environment variable
/// 4. `.env` file (handled by caller via dotenvy)
pub fn resolve_key(explicit: &Option<String>) -> Option<String> {
    if let Some(k) = explicit {
        if !k.is_empty() {
            return Some(k.clone());
        }
    }
    if let Some(k) = load_key() {
        return Some(k);
    }
    std::env::var("XC_API_KEY").ok().filter(|k| !k.is_empty())
}
