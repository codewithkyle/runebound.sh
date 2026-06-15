use std::sync::Arc;

use crate::app_state::AppState;
use crate::commands::{ok_response, DesktopHandlerInvocation};
use crate::entities::domains::{npc_event_from_draft, npc_summary_text};
use crate::entities::{EntityDomain, EntityKind};
use crate::services::entity_admin::{EntityAdminService, EnsureLocationInput};
use crate::utils::normalize_optional_prompt;
use runebound_models::CommandResponse;

pub async fn handle_npc(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
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
            return Ok(Some(ok_response(
                "no active npc draft. run create npc or load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(domain.help_text(), None)));
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

    if lowered == "npc reroll" || lowered.starts_with("npc reroll ") {
        return handle_npc_reroll(trimmed, state_ref, &domain).await;
    }

    Ok(Some(ok_response(
        "unknown npc command. use `npc help`".to_string(),
        None,
    )))
}

async fn handle_npc_reroll(
    trimmed: &str,
    state: &AppState,
    domain: &Arc<dyn EntityDomain>,
) -> Result<Option<CommandResponse>, String> {
    if trimmed.eq_ignore_ascii_case("npc reroll") {
        return Ok(Some(ok_response(
            "usage: npc reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    if trimmed.len() <= 11 {
        return Ok(Some(ok_response(
            "usage: npc reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let args = trimmed[11..].trim();
    if args.is_empty() {
        return Ok(Some(ok_response(
            "usage: npc reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let mut split = args.splitn(2, char::is_whitespace);
    let field = split.next().unwrap_or_default().trim().to_string();
    if field.is_empty() {
        return Ok(Some(ok_response(
            "usage: npc reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let prompt = normalize_optional_prompt(split.next().map(|value| value.to_string()));
    domain.reroll_field(&field, prompt, state).await
}

async fn npc_travel(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    if !trimmed.to_ascii_lowercase().starts_with("npc travel to ") {
        return Ok(Some(ok_response(
            "usage: npc travel to <location>".to_string(),
            None,
        )));
    }
    let location_name = trimmed[14..].trim();
    if location_name.is_empty() {
        return Ok(Some(ok_response(
            "location cannot be empty.".to_string(),
            None,
        )));
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

    Ok(Some(ok_response(
        npc_summary_text(&draft),
        Some(npc_event_from_draft(&draft)),
    )))
}

fn npc_domain(state: &AppState) -> Arc<dyn EntityDomain> {
    state
        .domains()
        .domain(EntityKind::Npc)
        .expect("npc domain not registered")
}
