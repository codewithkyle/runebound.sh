use crate::app_state::{AppState, DraftEnvelope};
use crate::commands::{DesktopHandlerInvocation, ok_response, ok_response_with_doc};
use dnd_core::command::CommandClientEvent;
use runebound_models::{CommandResponse, OutputDoc};

use crate::entities::EntityDetail;
use crate::entities::domains::{
    dungeon_event_from_draft, event_event_from_draft, faction_event_from_draft,
    god_event_from_draft, item_event_from_draft, location_event_from_draft, npc_event_from_draft,
};
use crate::services::entity_admin::{EntityAdminService, SoftDeleteEntityInput};
use crate::utils::path_for_display;

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
        format!("trash: {}", path_for_display(&result.trash_vault_path)),
        "tip: run undo to restore it.".to_string(),
    ]
    .join("\n");

    let should_clear = {
        let editor = invocation.state.editor_session.lock().await;
        editor.get_npc().is_some_and(|draft| draft.id == result.id)
            || editor
                .get_location()
                .is_some_and(|draft| draft.id == result.id)
            || editor
                .get_faction()
                .is_some_and(|draft| draft.id == result.id)
            || editor.get_god().is_some_and(|draft| draft.id == result.id)
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
fn build_entity_card_text(detail: &EntityDetail) -> String {
    card_doc_to_text(&build_entity_card_doc(detail))
}

fn card_doc_to_text(doc: &OutputDoc) -> String {
    use runebound_models::{InlineNode, OutputBlock};

    fn inlines_to_text(inlines: &[InlineNode]) -> String {
        inlines
            .iter()
            .map(|node| match node {
                InlineNode::Text { text }
                | InlineNode::Emphasis { text }
                | InlineNode::Strong { text }
                | InlineNode::Code { text } => text.clone(),
                InlineNode::CommandRef { label, .. } => label.clone(),
            })
            .collect()
    }

    let mut lines: Vec<String> = Vec::new();
    for block in &doc.blocks {
        match block {
            OutputBlock::Heading { text, .. } => lines.push(format!("## {text}")),
            OutputBlock::Paragraph { inlines } => lines.push(inlines_to_text(inlines)),
            OutputBlock::Status { text, .. } | OutputBlock::Code { text, .. } => {
                lines.push(text.clone())
            }
            OutputBlock::List { items } => {
                for item in items {
                    lines.push(format!("- {}", inlines_to_text(item)));
                }
            }
            OutputBlock::EntityCard { title, rows } => {
                lines.push(format!("## {title}"));
                for row in rows {
                    lines.push(format!("{} {}", row.label, row.value));
                }
            }
            OutputBlock::Spinner { text, .. } => lines.push(text.clone()),
            OutputBlock::Image { alt, .. } => lines.push(alt.clone()),
        }
    }
    lines.join("\n")
}
