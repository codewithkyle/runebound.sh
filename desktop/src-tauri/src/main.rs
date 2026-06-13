#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use dnd_core::command::CommandResponse;
use dnd_core::command_manifest::{CommandManifest, command_manifest};
use dnd_core::command_parse::{ParseResult, parse_command_input as parse_shared_command_input};

struct AppState {
    workspace_root: PathBuf,
}

#[tauri::command]
async fn run_command(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<CommandResponse, String> {
    Ok(dnd_core::command::execute_line(&state.workspace_root, &input).await)
}

#[tauri::command]
fn get_command_manifest() -> CommandManifest {
    command_manifest()
}

#[tauri::command]
fn parse_command_input(input: String) -> ParseResult {
    parse_shared_command_input(&input)
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn main() {
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    tauri::Builder::default()
        .manage(AppState { workspace_root })
        .invoke_handler(tauri::generate_handler![
            run_command,
            get_command_manifest,
            parse_command_input,
            exit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
