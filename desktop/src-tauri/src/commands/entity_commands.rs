use crate::app_state::{AppState, DraftEnvelope};
use crate::commands::{
    DesktopHandlerInvocation, command_action_response, ok_response, ok_response_with_doc,
};
use dnd_core::command::CommandClientEvent;
use runebound_models::{CommandResponse, OutputDoc};

use crate::entities::common::{
    command_message_response, command_message_response_with_doc, command_no_active_draft,
    command_response_with_event, entity_help_doc, entity_reroll_field_help, entity_set_field_help,
    parse_reroll_field_and_prompt,
};
use crate::entities::domains::{
    dungeon_event_from_draft, event_event_from_draft, faction_event_from_draft,
    god_event_from_draft, item_event_from_draft, location_event_from_draft, npc_event_from_draft,
    npc_summary_text,
};
use crate::entities::schema::{rerollable_fields, settable_fields};
use crate::entities::{CommandResult, EntityDetail, EntityKind};
use crate::services::entity_admin::{
    EnsureLocationInput, EntityAdminService, SoftDeleteEntityInput,
};
use crate::utils::{normalize_optional_prompt, path_for_display};

pub async fn handle_load(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "load" {
        return Ok(Some(ok_response(
            "usage: load <npc-or-location-or-faction-name>".to_string(),
            None,
        )));
    }
    if !lowered.starts_with("load ") {
        return Ok(None);
    }

    let target = trimmed[4..].trim();
    if target.is_empty() {
        return Ok(Some(ok_response(
            "usage: load <npc-or-location-or-faction-name>".to_string(),
            None,
        )));
    }

    let admin = EntityAdminService;
    let entity = admin
        .resolve_entity(target.to_string(), invocation.state.inner())
        .await?;
    let Some(entity) = entity else {
        return Ok(Some(ok_response(
            format!("no npc, location, or faction found for: {target}"),
            None,
        )));
    };

    let (output, event) = build_load_response(entity, invocation.state.clone()).await;
    Ok(Some(ok_response(output, event)))
}

pub async fn handle_show(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    entity_preview_response(invocation, "show").await
}

pub async fn handle_preview(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    entity_preview_response(invocation, "preview").await
}

async fn entity_preview_response(
    invocation: DesktopHandlerInvocation<'_>,
    root: &str,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    if lowered == root {
        return Ok(Some(ok_response(
            format!("usage: {} <npc-or-location-or-faction-name>", root),
            None,
        )));
    }
    if !lowered.starts_with(&format!("{root} ")) {
        return Ok(None);
    }
    let target = trimmed[root.len()..].trim();
    if target.is_empty() {
        return Ok(Some(ok_response(
            format!("usage: {} <npc-or-location-or-faction-name>", root),
            None,
        )));
    }
    let admin = EntityAdminService;
    let entity = admin
        .resolve_entity(target.to_string(), invocation.state.inner())
        .await?;
    let Some(entity) = entity else {
        return Ok(Some(ok_response(
            format!("no npc, location, or faction found for: {target}"),
            None,
        )));
    };

    let preview_text = build_preview_response(&entity);
    let preview_doc = build_entity_card_doc(&entity);
    Ok(Some(ok_response_with_doc(
        preview_text,
        Some(preview_doc),
        None,
    )))
}

pub async fn handle_delete(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    if lowered == "delete" {
        return Ok(Some(ok_response(
            "usage: delete <npc-or-location-or-faction-name>".to_string(),
            None,
        )));
    }
    if !lowered.starts_with("delete ") {
        return Ok(None);
    }
    let target = trimmed[6..].trim();
    if target.is_empty() {
        return Ok(Some(ok_response(
            "usage: delete <npc-or-location-or-faction-name>".to_string(),
            None,
        )));
    }

    let admin = EntityAdminService;
    let result = admin
        .soft_delete_entity(
            SoftDeleteEntityInput {
                target: target.to_string(),
            },
            invocation.state.inner(),
        )
        .await?;

    let output = [
        "## Deleted".to_string(),
        format!("type: {}", result.entity_type.as_str()),
        format!("name: {}", result.name),
        format!("slug: {}", result.slug),
        "tip: run undo to restore it.".to_string(),
    ]
    .join("\n");

    let should_clear = {
        let editor = invocation.state.editor_session.lock().await;
        editor
            .active_draft()
            .is_some_and(|draft| draft.id() == result.id)
    };

    if should_clear {
        let mut editor = invocation.state.editor_session.lock().await;
        editor.clear_all();
        return Ok(Some(ok_response(
            output,
            Some(CommandClientEvent::ClearDrafts),
        )));
    }

    Ok(Some(ok_response(output, None)))
}

pub async fn handle_undo(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let admin = EntityAdminService;
    let result = admin
        .undo_last_soft_delete(invocation.state.inner())
        .await?;
    let output = [
        "## Undo complete".to_string(),
        format!("type: {}", result.entity_type.as_str()),
        format!("name: {}", result.name),
        format!("slug: {}", result.slug),
        format!("vault: {}", path_for_display(&result.vault_path)),
    ]
    .join("\n");
    Ok(Some(ok_response(output, None)))
}

pub(crate) async fn build_load_response(
    detail: EntityDetail,
    state: tauri::State<'_, AppState>,
) -> (String, Option<CommandClientEvent>) {
    let event = draft_loaded_event(&detail.draft);
    let text = build_entity_card_text(&detail);
    {
        let mut editor = state.editor_session.lock().await;
        editor.set_active_draft(detail.draft);
    }
    (text, Some(event))
}

/// The event that loads a freshly-resolved draft into the editor with its card.
/// One match over the draft envelope, delegating to the per-kind event builders
/// (which already carry the canonical entity card).
fn draft_loaded_event(draft: &DraftEnvelope) -> CommandClientEvent {
    match draft {
        DraftEnvelope::Npc(d) => npc_event_from_draft(d),
        DraftEnvelope::Location(d) => location_event_from_draft(d),
        DraftEnvelope::Faction(d) => faction_event_from_draft(d),
        DraftEnvelope::Item(d) => item_event_from_draft(d),
        DraftEnvelope::Event(d) => event_event_from_draft(d),
        DraftEnvelope::God(d) => god_event_from_draft(d),
        DraftEnvelope::Dungeon(d) => dungeon_event_from_draft(d),
    }
}

fn build_preview_response(detail: &EntityDetail) -> String {
    build_entity_card_text(detail)
}

/// The card for `show`/`preview`/`load` is the canonical entity card built from
/// the typed draft — the single source shared with the draft-edit path, so the
/// per-kind field list is no longer re-encoded here. (P5.4)
fn build_entity_card_doc(detail: &EntityDetail) -> OutputDoc {
    use runebound_models::drafts::{
        dungeon_entity_card, event_entity_card, faction_entity_card, god_entity_card,
        item_entity_card, location_entity_card, npc_entity_card,
    };
    match &detail.draft {
        DraftEnvelope::Npc(d) => npc_entity_card(d),
        DraftEnvelope::Location(d) => location_entity_card(d),
        DraftEnvelope::Faction(d) => faction_entity_card(d),
        DraftEnvelope::Item(d) => item_entity_card(d),
        DraftEnvelope::Event(d) => event_entity_card(d),
        DraftEnvelope::God(d) => god_entity_card(d),
        DraftEnvelope::Dungeon(d) => dungeon_entity_card(d),
    }
}

/// Plain-text fallback for the card, derived from the same `OutputDoc` the
/// frontend renders — so the doc is the single source and the text can't drift.
/// Shares one doc→text renderer with the core help surfaces (P7.2).
fn build_entity_card_text(detail: &EntityDetail) -> String {
    build_entity_card_doc(detail).to_plain_text()
}

/// Generic handler for every entity's `<root> ...` editor command ladder (P5.3).
/// The seven per-kind command modules were character-for-character identical
/// modulo the root string and a hand-counted rename byte offset; this drives the
/// ladder off `kind.command_root()` and the schema (so rename/set parse off the
/// real prefix length, no magic offsets). Registered per kind in `commands/mod.rs`.
pub async fn dispatch_entity_command(
    kind: EntityKind,
    invocation: DesktopHandlerInvocation<'_>,
) -> CommandResult {
    let root = kind.command_root();
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let state_ref = invocation.state.inner();
    let domain = state_ref
        .domains()
        .domain(kind)
        .unwrap_or_else(|| panic!("{root} domain not registered"));

    // Events are narrative-only (empty schema): no set/rename, and `reroll`
    // regenerates the whole body rather than a named field. Every other kind has
    // both settable and rerollable fields.
    let has_fields = settable_fields(kind).next().is_some();
    let per_field_reroll = rerollable_fields(kind).next().is_some();

    if lowered == format!("{root} help") {
        let has_draft = {
            let editor = state_ref.editor_session.lock().await;
            editor.draft(kind).is_some()
        };
        if !has_draft {
            return command_no_active_draft(kind);
        }
        let prose = domain.help_text();
        let help_doc = entity_help_doc(kind, &prose);
        return command_message_response_with_doc(prose, help_doc);
    }

    if lowered == format!("{root} show") {
        return domain.show_draft(state_ref).await;
    }

    if lowered == format!("{root} cancel") {
        return domain.cancel(state_ref).await;
    }

    // `npc travel to <location>` is the one per-kind extra verb.
    if kind == EntityKind::Npc && lowered.starts_with("npc travel ") {
        return npc_travel(trimmed, invocation.state.clone()).await;
    }

    if has_fields {
        let rename_prefix = format!("{root} rename ");
        if lowered.starts_with(&rename_prefix) {
            let name = trimmed[rename_prefix.len()..].trim();
            return domain.rename(name, state_ref).await;
        }

        if lowered == format!("{root} set help") {
            return entity_set_field_help(kind);
        }

        let set_prefix = format!("{root} set ");
        if lowered.starts_with(&set_prefix) {
            // splitn(4) keeps the value as one token so multi-word values (and beat
            // edits like `dungeon set setback loot none`) reach the domain intact.
            let mut parts = trimmed.splitn(4, char::is_whitespace);
            let _ = parts.next();
            let _ = parts.next();
            let field = parts.next().unwrap_or_default();
            let value = parts.next().unwrap_or_default();
            return domain.set_field(field, value, state_ref).await;
        }
    }

    if lowered == format!("{root} save") {
        return domain.save(state_ref).await;
    }

    let reroll_word = format!("{root} reroll");
    let is_reroll = lowered == reroll_word || lowered.starts_with(&format!("{reroll_word} "));
    if per_field_reroll {
        if lowered == format!("{root} reroll help") {
            return entity_reroll_field_help(kind);
        }
        if is_reroll {
            let usage = if kind == EntityKind::Dungeon {
                "usage: dungeon reroll <beat>|premise|name [prompt]".to_string()
            } else {
                format!("usage: {root} reroll <field> [prompt]")
            };
            let (field, prompt) = match parse_reroll_field_and_prompt(trimmed, &reroll_word, &usage)
            {
                Ok(result) => result,
                Err(response) => return response,
            };
            return domain.reroll_field(&field, prompt, state_ref).await;
        }
    } else if is_reroll {
        // Narrative-only reroll (events): text after `reroll` is free-form guidance,
        // not a field name; the empty field argument is ignored by the domain.
        let prompt = normalize_optional_prompt(Some(trimmed[reroll_word.len()..].to_string()));
        return domain.reroll_field("", prompt, state_ref).await;
    }

    Ok(Some(command_action_response(
        &format!("unknown {root} command. use "),
        &format!("{root} help"),
        "",
    )))
}

/// `npc travel to <location>`: the one entity verb outside the generic ladder.
/// Ensures the named location exists (creating a stub if needed) and points the
/// active NPC draft at its canonical name.
async fn npc_travel(trimmed: &str, state: tauri::State<'_, AppState>) -> CommandResult {
    if !trimmed.to_ascii_lowercase().starts_with("npc travel to ") {
        return command_message_response("usage: npc travel to <location>");
    }
    let location_name = trimmed["npc travel to ".len()..].trim();
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
    }

    command_response_with_event(npc_summary_text(&draft), npc_event_from_draft(&draft))
}
