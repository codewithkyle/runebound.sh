use command_handler::CommandHandler;
use dnd_core::command_manifest::command_manifest;
use runebound_models::CommandResponse;
use tauri::State;

use crate::app_state::AppState;
use crate::commands::{desktop_handler_registry, ok_response, DesktopHandlerInvocation};
use crate::commands::entity_commands::build_load_response;
use crate::services::entity_admin::{EntityAdminService, EntityDetails};
use crate::services::suggestions::starts_with_known_command_root;

pub(crate) async fn dispatch_desktop_command(
    input: &str,
    tokens: &[String],
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<Option<CommandResponse>, String> {
    if tokens.is_empty() {
        return Ok(None);
    }

    let lowered: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();

    let registry = desktop_handler_registry();
    if let Some(entry) = registry.get(lowered[0].as_str()) {
        let invocation = DesktopHandlerInvocation {
            raw_input: input,
            tokens,
            lowered: &lowered,
            state: state.clone(),
            app_handle: app_handle.clone(),
        };
        return entry.execute(invocation).await;
    }

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let manifest = command_manifest();
    if !starts_with_known_command_root(trimmed, &manifest) {
        let admin = EntityAdminService;
        if let Some(entity) = admin
            .resolve_entity(trimmed.to_string(), state.inner())
            .await?
        {
            let load_entity: EntityDetails = entity;
            let (output, event) = build_load_response(load_entity, state).await;
            return Ok(Some(ok_response(output, event)));
        }
    }

    Ok(None)
}
