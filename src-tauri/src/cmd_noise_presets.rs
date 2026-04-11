use tauri::Manager;

#[tauri::command]
pub fn save_noise_preset(app: tauri::AppHandle, name: String, json: String) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("noise-presets");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect();
    let sanitized = sanitized.trim().to_string();
    let filename = if sanitized.is_empty() {
        "noise_profile.batm".to_string()
    } else {
        format!("{}.batm", sanitized.replace(' ', "_").to_lowercase())
    };
    let path = dir.join(&filename);
    std::fs::write(&path, &json).map_err(|e| e.to_string())?;
    Ok(filename)
}

#[tauri::command]
pub fn load_noise_preset(app: tauri::AppHandle, name: String) -> Result<String, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("noise-presets")
        .join(&name);
    std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read preset '{}': {}", name, e))
}

#[tauri::command]
pub fn list_noise_presets(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("noise-presets");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut presets: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".batm") || name.ends_with(".json") { Some(name) } else { None }
        })
        .collect();
    presets.sort();
    Ok(presets)
}

#[tauri::command]
pub fn delete_noise_preset(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("noise-presets")
        .join(&name);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
