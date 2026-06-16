use std::sync::Arc;

use crate::app_state::AppState;
use crate::commands::{DesktopHandlerInvocation, command_action_response};
use crate::entities::common::{
    command_message_response,
    command_message_response_with_doc,
    command_no_active_draft,
    command_response_with_event,
    entity_help_doc,
    entity_reroll_field_help,
    entity_set_field_help,
    parse_reroll_field_and_prompt,
};
use crate::entities::domains::{npc_event_from_draft, npc_summary_text};
use crate::entities::{CommandResult, EntityDomain, EntityKind};
use crate::services::entity_admin::{EntityAdminService, EnsureLocationInput};

pub async fn handle_npc(
    invocation: DesktopHandlerInvocation<'_>,
) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let state_ref = invocation.state.inner();
    let domain = npc_domain(state_ref);

    if lowered == "npc help" {
        let has_draft = {
            let editor = state_ref.editor_session.lock().await;
            editor.get_npc().is_some()
        };
        if !has_draft {
            return command_no_active_draft(EntityKind::Npc);
        }
        let prose = domain.help_text();
        let help_doc = entity_help_doc(EntityKind::Npc, &prose);
        return command_message_response_with_doc(prose, help_doc);
    }

    if lowered == "npc show" {
        return domain.show_draft(state_ref).await;
    }

    if lowered == "npc cancel" {
        return domain.cancel(state_ref).await;
    }

    if lowered.starts_with("npc rename ") {
        let name = trimmed[10..].trim();
        return domain.rename(name, state_ref).await;
    }

    if lowered == "npc set help" {
        return entity_set_field_help(EntityKind::Npc);
    }

    if lowered.starts_with("npc set ") {
        let mut parts = trimmed.splitn(4, char::is_whitespace);
        let _ = parts.next();
        let _ = parts.next();
        let field = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        return domain.set_field(field, value, state_ref).await;
    }

    if lowered.starts_with("npc travel ") {
        return npc_travel(trimmed, invocation.state.clone()).await;
    }

    if lowered == "npc save" {
        return domain.save(state_ref).await;
    }

    if lowered == "npc reroll help" {
        return entity_reroll_field_help(EntityKind::Npc);
    }

    if lowered == "npc reroll" || lowered.starts_with("npc reroll ") {
        return handle_npc_reroll(trimmed, state_ref, &domain).await;
    }

    Ok(Some(command_action_response(
        "unknown npc command. use ",
        "npc help",
        "",
    )))
}

async fn handle_npc_reroll(
    trimmed: &str,
    state: &AppState,
    domain: &Arc<dyn EntityDomain>,
) -> CommandResult {
    let (field, prompt) = match parse_reroll_field_and_prompt(
        trimmed,
        "npc reroll",
        "usage: npc reroll <field> [prompt]",
    ) {
        Ok(result) => result,
        Err(response) => return response,
    };
    domain.reroll_field(&field, prompt, state).await
}

async fn npc_travel(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> CommandResult {
    if !trimmed.to_ascii_lowercase().starts_with("npc travel to ") {
        return command_message_response("usage: npc travel to <location>");
    }
    let location_name = trimmed[14..].trim();
    if location_name.is_empty() {
        return command_message_response("location cannot be empty.");
    }

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.get_npc().cloned()
    }
    .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

    let admin = EntityAdminService;
    let result = admin
        .ensure_location_exists(
            EnsureLocationInput {
                name: location_name.to_string(),
            },
            state.inner(),
        )
        .await?;
    draft.location = if result.name.trim().is_empty() {
        location_name.to_string()
    } else {
        result.name
    };

    {
        let mut editor = state.editor_session.lock().await;
        editor.set_npc(draft.clone());
        editor.clear_kind(EntityKind::Location);
    }

    command_response_with_event(npc_summary_text(&draft), npc_event_from_draft(&draft))
}

fn npc_domain(state: &AppState) -> Arc<dyn EntityDomain> {
    state
        .domains()
        .domain(EntityKind::Npc)
        .expect("npc domain not registered")
}
