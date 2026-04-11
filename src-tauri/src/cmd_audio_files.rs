use crate::audio_decode::{self, AudioFileInfo, FullDecodeResult};

#[tauri::command]
pub fn audio_file_info(path: String) -> Result<AudioFileInfo, String> {
    audio_decode::file_info(&path)
}

#[tauri::command]
pub fn audio_decode_full(path: String) -> Result<FullDecodeResult, String> {
    audio_decode::decode_full(&path)
}

/// Read raw file bytes — returns binary data via efficient IPC (no JSON serialization).
#[tauri::command]
pub fn read_file_bytes(path: String) -> Result<tauri::ipc::Response, String> {
    let bytes = std::fs::read(&path)
        .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Read a byte range from a file — for streaming large files without loading entirely.
#[tauri::command]
pub fn read_file_range(path: String, offset: u64, length: u64) -> Result<tauri::ipc::Response, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(&path)
        .map_err(|e| format!("Failed to open '{}': {}", path, e))?;
    f.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("Seek failed: {}", e))?;
    let mut buf = vec![0u8; length as usize];
    f.read_exact(&mut buf)
        .map_err(|e| format!("Read failed: {}", e))?;
    Ok(tauri::ipc::Response::new(buf))
}
