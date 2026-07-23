use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemWritableFileStream, WritableStream,
};

use crate::project::BatProject;

const OPFS_PROJECTS_DIR: &str = "oversample-projects";

/// Get the OPFS oversample-projects directory, creating it if needed.
async fn get_projects_dir() -> Result<FileSystemDirectoryHandle, String> {
    let window = web_sys::window().ok_or("no window")?;
    let navigator = window.navigator();
    let storage = navigator.storage();
    let root: FileSystemDirectoryHandle = JsFuture::from(storage.get_directory())
        .await
        .map_err(|e| format!("OPFS root: {e:?}"))?
        .unchecked_into();

    let opts = web_sys::FileSystemGetDirectoryOptions::new();
    opts.set_create(true);
    let dir: FileSystemDirectoryHandle =
        JsFuture::from(root.get_directory_handle_with_options(OPFS_PROJECTS_DIR, &opts))
            .await
            .map_err(|e| format!("OPFS projects dir: {e:?}"))?
            .unchecked_into();
    Ok(dir)
}

/// Storage key for a project file.
fn project_key(id: &str) -> String {
    format!("{}.batproj", id)
}

/// Save a project to OPFS as YAML.
pub async fn save_project(project: &BatProject) -> Result<(), String> {
    let yaml = yaml_serde::to_string(project).map_err(|e| format!("YAML serialize: {e}"))?;

    let dir = get_projects_dir().await?;
    let key = project_key(&project.id);

    let opts = web_sys::FileSystemGetFileOptions::new();
    opts.set_create(true);
    let file_handle: FileSystemFileHandle =
        JsFuture::from(dir.get_file_handle_with_options(&key, &opts))
            .await
            .map_err(|e| format!("OPFS get file: {e:?}"))?
            .unchecked_into();

    let writable: FileSystemWritableFileStream = JsFuture::from(file_handle.create_writable())
        .await
        .map_err(|e| format!("OPFS create writable: {e:?}"))?
        .unchecked_into();

    JsFuture::from(
        writable
            .write_with_str(&yaml)
            .map_err(|e| format!("OPFS write: {e:?}"))?,
    )
    .await
    .map_err(|e| format!("OPFS write await: {e:?}"))?;

    let ws: &WritableStream = writable.unchecked_ref();
    JsFuture::from(ws.close())
        .await
        .map_err(|e| format!("OPFS close: {e:?}"))?;

    Ok(())
}

/// Load a project from OPFS by its ID. Returns None if not found.
pub async fn load_project(id: &str) -> Result<Option<BatProject>, String> {
    let dir = get_projects_dir().await?;
    let key = project_key(id);

    let file_handle_result = JsFuture::from(dir.get_file_handle(&key)).await;
    let file_handle: FileSystemFileHandle = match file_handle_result {
        Ok(h) => h.unchecked_into(),
        Err(_) => return Ok(None),
    };

    let file: web_sys::File = JsFuture::from(file_handle.get_file())
        .await
        .map_err(|e| format!("OPFS get file: {e:?}"))?
        .unchecked_into();

    let text = JsFuture::from(file.text())
        .await
        .map_err(|e| format!("OPFS read text: {e:?}"))?;

    let yaml_str = text.as_string().ok_or("OPFS text not a string")?;
    let project: BatProject =
        yaml_serde::from_str(&yaml_str).map_err(|e| format!("YAML deserialize: {e}"))?;

    if project.version > crate::project::PROJECT_FORMAT_VERSION {
        log::warn!(
            "Project {id} is format version {} but this app supports {}; loading \
             best-effort — some data may be missing or misread.",
            project.version,
            crate::project::PROJECT_FORMAT_VERSION
        );
    }

    Ok(Some(project))
}

/// Summary info for a saved project (for the list picker).
#[derive(Clone)]
pub struct ProjectSummary {
    pub id: String,
    pub name: Option<String>,
    pub file_count: usize,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
}

/// List all project IDs stored in OPFS.
/// Returns Vec of project summaries.
pub async fn list_projects() -> Result<Vec<ProjectSummary>, String> {
    let dir = get_projects_dir().await?;
    let mut result = Vec::new();

    // Iterate directory entries via JS async iterator
    let entries = dir.entries();
    loop {
        let next = JsFuture::from(entries.next().map_err(|e| format!("iter: {e:?}"))?)
            .await
            .map_err(|e| format!("iter next: {e:?}"))?;

        let done = js_sys::Reflect::get(&next, &"done".into())
            .map_err(|e| format!("done: {e:?}"))?
            .as_bool()
            .unwrap_or(true);
        if done {
            break;
        }

        let value =
            js_sys::Reflect::get(&next, &"value".into()).map_err(|e| format!("value: {e:?}"))?;
        let arr = js_sys::Array::from(&value);
        let key_js = arr.get(0);
        let key = key_js.as_string().unwrap_or_default();

        if key.ends_with(".batproj") {
            let id = key.trim_end_matches(".batproj").to_string();
            match load_project(&id).await {
                Ok(Some(proj)) => result.push(ProjectSummary {
                    id,
                    name: proj.name,
                    file_count: proj.files.len(),
                    created_at: proj.created_at,
                    modified_at: proj.modified_at,
                }),
                _ => result.push(ProjectSummary {
                    id,
                    name: None,
                    file_count: 0,
                    created_at: None,
                    modified_at: None,
                }),
            }
        }
    }

    // Sort by modified_at descending (most recent first). modified_at is ISO-8601,
    // which is lexically sortable in chronological order, so a plain string compare
    // is correct; projects with no timestamp (None < Some) consistently sort last.
    result.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));

    Ok(result)
}

/// Delete a project from OPFS by ID.
pub async fn delete_project(id: &str) -> Result<(), String> {
    let dir = get_projects_dir().await?;
    let key = project_key(id);

    JsFuture::from(dir.remove_entry(&key))
        .await
        .map_err(|e| format!("OPFS delete: {e:?}"))?;

    Ok(())
}

/// Export a project as a YAML string (for download).
pub fn export_project_yaml(project: &BatProject) -> Result<String, String> {
    yaml_serde::to_string(project).map_err(|e| format!("YAML serialize: {e}"))
}
