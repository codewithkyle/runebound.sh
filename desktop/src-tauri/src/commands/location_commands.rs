use std::sync::Arc;

use crate::app_state::AppState;
use crate::commands::DesktopHandlerInvocation;
use crate::entities::common::{
    command_message_response,
    command_no_active_draft,
    entity_reroll_field_help,
    entity_set_field_help,
    parse_reroll_field_and_prompt,
};
use crate::entities::{CommandResult, EntityDomain, EntityKind};

pub async fn handle_location(
    invocation: DesktopHandlerInvocation<'_>,
) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let state_ref = invocation.state.inner();
    let domain = location_domain(state_ref);

    if lowered == "location help" {
        let has_draft = {
            let editor = state_ref.editor_session.lock().await;
            editor.get_location().is_some()
        };
        if !has_draft {
            return command_no_active_draft(EntityKind::Location);
        }
        return command_message_response(domain.help_text());
    }

    if lowered == "location show" {
        return domain.show_draft(state_ref).await;
    }

    if lowered == "location cancel" {
        return domain.cancel(state_ref).await;
    }

    if lowered.starts_with("location rename ") {
        let name = trimmed[16..].trim();
        return domain.rename(name, state_ref).await;
    }

    if lowered == "location set help" {
        return entity_set_field_help(EntityKind::Location);
    }

    if lowered.starts_with("location set ") {
        let mut parts = trimmed.splitn(4, char::is_whitespace);
        let _ = parts.next();
        let _ = parts.next();
        let field = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        return domain.set_field(field, value, state_ref).await;
    }

    if lowered == "location save" {
        return domain.save(state_ref).await;
    }

    if lowered == "location reroll help" {
        return entity_reroll_field_help(EntityKind::Location);
    }

    if lowered == "location reroll" || lowered.starts_with("location reroll ") {
        return handle_location_reroll(trimmed, state_ref, &domain).await;
    }

    command_message_response("unknown location command. use `location help`")
}

async fn handle_location_reroll(
    trimmed: &str,
    state: &AppState,
    domain: &Arc<dyn EntityDomain>,
) -> CommandResult {
    let (field, prompt) = match parse_reroll_field_and_prompt(
        trimmed,
        "location reroll",
        "usage: location reroll <field> [prompt]",
    ) {
        Ok(result) => result,
        Err(response) => return response,
    };
    domain.reroll_field(&field, prompt, state).await
}

fn location_domain(state: &AppState) -> Arc<dyn EntityDomain> {
    state
        .domains()
        .domain(EntityKind::Location)
        .expect("location domain not registered")
}
