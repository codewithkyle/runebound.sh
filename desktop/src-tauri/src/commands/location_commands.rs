use std::sync::Arc;

use crate::app_state::AppState;
use crate::commands::{ok_response, DesktopHandlerInvocation};
use crate::entities::{EntityDomain, EntityKind};
use crate::utils::normalize_optional_prompt;
use runebound_models::CommandResponse;

pub async fn handle_location(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
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
            return Ok(Some(ok_response(
                "no active location draft. run create location or load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(domain.help_text(), None)));
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

    if lowered == "location reroll" || lowered.starts_with("location reroll ") {
        return handle_location_reroll(trimmed, state_ref, &domain).await;
    }

    Ok(Some(ok_response(
        "unknown location command. use `location help`".to_string(),
        None,
    )))
}

async fn handle_location_reroll(
    trimmed: &str,
    state: &AppState,
    domain: &Arc<dyn EntityDomain>,
) -> Result<Option<CommandResponse>, String> {
    if trimmed.eq_ignore_ascii_case("location reroll") {
        return Ok(Some(ok_response(
            "usage: location reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    if trimmed.len() <= 16 {
        return Ok(Some(ok_response(
            "usage: location reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let args = trimmed[16..].trim();
    if args.is_empty() {
        return Ok(Some(ok_response(
            "usage: location reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let mut split = args.splitn(2, char::is_whitespace);
    let field = split.next().unwrap_or_default().trim().to_string();
    if field.is_empty() {
        return Ok(Some(ok_response(
            "usage: location reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let prompt = normalize_optional_prompt(split.next().map(|value| value.to_string()));
    domain.reroll_field(&field, prompt, state).await
}

fn location_domain(state: &AppState) -> Arc<dyn EntityDomain> {
    state
        .domains()
        .domain(EntityKind::Location)
        .expect("location domain not registered")
}
