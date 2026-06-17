use std::path::PathBuf;

use tauri_plugin_dialog::{DialogExt, FilePath};

/// Result of asking the user to choose a vault directory with the native picker.
pub enum FolderPick {
    Picked(String),
    Cancelled,
}

/// Open the native folder picker so the user can choose their Obsidian vault.
///
/// Mirrors the file-picker pattern used for calendar import (see
/// `calendar_commands.rs`), but selects a directory instead of a file and
/// expands the result so it matches the paths the core onboarding flow expects.
pub fn pick_vault_folder(app_handle: &tauri::AppHandle) -> Result<FolderPick, String> {
    let picked = app_handle
        .dialog()
        .file()
        .set_title("Select Obsidian Vault")
        .blocking_pick_folder();

    let path = match picked {
        None => return Ok(FolderPick::Cancelled),
        Some(FilePath::Path(p)) => p,
        Some(FilePath::Url(u)) => PathBuf::from(u.as_str()),
    };

    let path_string = path.to_string_lossy().into_owned();
    let expanded =
        shellexpand::full(&path_string).map_err(|e| format!("failed to expand path: {}", e))?;

    Ok(FolderPick::Picked(expanded.into_owned()))
}
