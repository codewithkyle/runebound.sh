use std::sync::Arc;

use crate::app_state::AppState;
use crate::commands::{ok_response, DesktopHandlerInvocation};
use crate::entities::{EntityDomain, EntityKind};
use crate::utils::normalize_optional_prompt;
use runebound_models::CommandResponse;

pub async fn handle_faction(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
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
            return Ok(Some(ok_response(
                "no active faction draft. run create faction or load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(domain.help_text(), None)));
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

    if lowered == "faction reroll" || lowered.starts_with("faction reroll ") {
        return handle_faction_reroll(trimmed, state_ref, &domain).await;
    }

    Ok(Some(ok_response(
        "unknown faction command. use `faction help`".to_string(),
        None,
    )))
}

async fn handle_faction_reroll(
    trimmed: &str,
    state: &AppState,
    domain: &Arc<dyn EntityDomain>,
) -> Result<Option<CommandResponse>, String> {
    if trimmed.eq_ignore_ascii_case("faction reroll") {
        return Ok(Some(ok_response(
            "usage: faction reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    if trimmed.len() <= 15 {
        return Ok(Some(ok_response(
            "usage: faction reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let args = trimmed[15..].trim();
    if args.is_empty() {
        return Ok(Some(ok_response(
            "usage: faction reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let mut split = args.splitn(2, char::is_whitespace);
    let field = split.next().unwrap_or_default().trim().to_string();
    if field.is_empty() {
        return Ok(Some(ok_response(
            "usage: faction reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let prompt = normalize_optional_prompt(split.next().map(|value| value.to_string()));
    domain.reroll_field(&field, prompt, state).await
}

fn faction_domain(state: &AppState) -> Arc<dyn EntityDomain> {
    state
        .domains()
        .domain(EntityKind::Faction)
        .expect("faction domain not registered")
}
