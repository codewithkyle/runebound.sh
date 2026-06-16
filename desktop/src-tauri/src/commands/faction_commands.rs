use std::sync::Arc;

use crate::app_state::AppState;
use crate::commands::{DesktopHandlerInvocation, command_action_response};
use crate::entities::common::{
    command_message_response_with_doc,
    command_no_active_draft,
    entity_help_doc,
    entity_reroll_field_help,
    entity_set_field_help,
    parse_reroll_field_and_prompt,
};
use crate::entities::{CommandResult, EntityDomain, EntityKind};

pub async fn handle_faction(
    invocation: DesktopHandlerInvocation<'_>,
) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let state_ref = invocation.state.inner();
    let domain = faction_domain(state_ref);

    if lowered == "faction help" {
        let has_draft = {
            let editor = state_ref.editor_session.lock().await;
            editor.get_faction().is_some()
        };
        if !has_draft {
            return command_no_active_draft(EntityKind::Faction);
        }
        let prose = domain.help_text();
        let help_doc = entity_help_doc(EntityKind::Faction, &prose);
        return command_message_response_with_doc(prose, help_doc);
    }

    if lowered == "faction show" {
        return domain.show_draft(state_ref).await;
    }

    if lowered == "faction cancel" {
        return domain.cancel(state_ref).await;
    }

    if lowered.starts_with("faction rename ") {
        let name = trimmed[15..].trim();
        return domain.rename(name, state_ref).await;
    }

    if lowered == "faction set help" {
        return entity_set_field_help(EntityKind::Faction);
    }

    if lowered.starts_with("faction set ") {
        let mut parts = trimmed.splitn(4, char::is_whitespace);
        let _ = parts.next();
        let _ = parts.next();
        let field = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        return domain.set_field(field, value, state_ref).await;
    }

    if lowered == "faction save" {
        return domain.save(state_ref).await;
    }

    if lowered == "faction reroll help" {
        return entity_reroll_field_help(EntityKind::Faction);
    }

    if lowered == "faction reroll" || lowered.starts_with("faction reroll ") {
        return handle_faction_reroll(trimmed, state_ref, &domain).await;
    }

    Ok(Some(command_action_response(
        "unknown faction command. use ",
        "faction help",
        "",
    )))
}

async fn handle_faction_reroll(
    trimmed: &str,
    state: &AppState,
    domain: &Arc<dyn EntityDomain>,
) -> CommandResult {
    let (field, prompt) = match parse_reroll_field_and_prompt(
        trimmed,
        "faction reroll",
        "usage: faction reroll <field> [prompt]",
    ) {
        Ok(result) => result,
        Err(response) => return response,
    };
    domain.reroll_field(&field, prompt, state).await
}

fn faction_domain(state: &AppState) -> Arc<dyn EntityDomain> {
    state
        .domains()
        .domain(EntityKind::Faction)
        .expect("faction domain not registered")
}
