use std::sync::Arc;

use crate::app_state::AppState;
use crate::commands::{DesktopHandlerInvocation, command_action_response};
use crate::entities::common::{
    command_message_response_with_doc, command_no_active_draft, entity_help_doc,
};
use crate::entities::{CommandResult, EntityDomain, EntityKind};
use crate::utils::normalize_optional_prompt;

pub async fn handle_event(invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let state_ref = invocation.state.inner();
    let domain = event_domain(state_ref);

    if lowered == "event help" {
        let has_draft = {
            let editor = state_ref.editor_session.lock().await;
            editor.get_event().is_some()
        };
        if !has_draft {
            return command_no_active_draft(EntityKind::Event);
        }
        let prose = domain.help_text();
        let help_doc = entity_help_doc(EntityKind::Event, &prose);
        return command_message_response_with_doc(prose, help_doc);
    }

    if lowered == "event show" {
        return domain.show_draft(state_ref).await;
    }

    if lowered == "event cancel" {
        return domain.cancel(state_ref).await;
    }

    if lowered == "event save" {
        return domain.save(state_ref).await;
    }

    // `event reroll [prompt]` regenerates the entire narrative. Events have no
    // fields, so any text after `reroll` is a free-form guidance prompt, not a
    // field name. The empty field argument is ignored by the domain.
    if lowered == "event reroll" || lowered.starts_with("event reroll ") {
        let prompt = normalize_optional_prompt(Some(trimmed["event reroll".len()..].to_string()));
        return domain.reroll_field("", prompt, state_ref).await;
    }

    Ok(Some(command_action_response(
        "unknown event command. use ",
        "event help",
        "",
    )))
}

fn event_domain(state: &AppState) -> Arc<dyn EntityDomain> {
    state
        .domains()
        .domain(EntityKind::Event)
        .expect("event domain not registered")
}
