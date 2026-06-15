use crate::app_state::{AppState, EditorMode};
use crate::commands::{ok_response, DesktopHandlerInvocation};
use crate::entities::{
    canonical_field_name,
    format_valid_field_list,
    EntityKind,
    FieldAccess,
};
use dnd_core::command::CommandClientEvent;
use runebound_models::CommandResponse;

use crate::services::entity_persistence::{EntityPersistenceService, SaveLocationDraftInput};
use crate::services::entity_reroll::{
    EntityRerollService, LocationRerollContext, RerollLocationFieldInput,
};
use crate::utils::{
    normalize_optional_prompt, path_for_display,
};
use crate::app_state::LocationDraftSession;

pub async fn handle_location(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "location help" {
        let has_draft = {
            let editor = invocation.state.editor_session.lock().await;
            editor.location_draft.is_some()
        };
        if !has_draft {
            return Ok(Some(ok_response(
                "no active location draft. run create location or load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            [
                "## Location editor commands",
                "location show",
                "location rename <name>",
                "location set <field> <value>",
                "location reroll <field> [prompt]",
                "reroll",
                "location save",
                "location cancel",
            ]
            .join("\n"),
            None,
        )));
    }

    if lowered == "location show" {
        let draft = {
            let editor = invocation.state.editor_session.lock().await;
            editor.location_draft.clone()
        };
        let Some(draft) = draft else {
            return Ok(Some(ok_response(
                "no active location draft. run create location or load <name>.".to_string(),
                None,
            )));
        };
        return Ok(Some(ok_response(location_summary_text(&draft), Some(location_event_from_draft(&draft)))));
    }

    if lowered == "location cancel" {
        let had_draft = {
            let mut editor = invocation.state.editor_session.lock().await;
            let had = editor.location_draft.is_some();
            if had {
                editor.location_draft = None;
                editor.mode = if editor.npc_draft.is_some() {
                    EditorMode::Npc
                } else if editor.faction_draft.is_some() {
                    EditorMode::Faction
                } else {
                    EditorMode::None
                };
            }
            had
        };
        if !had_draft {
            return Ok(Some(ok_response("no active location draft. run create location or load <name>.".to_string(), None)));
        }
        return Ok(Some(ok_response("location draft discarded.".to_string(), Some(CommandClientEvent::ClearDrafts))));
    }

    if lowered.starts_with("location rename ") {
        return location_rename(trimmed, invocation.state.clone()).await;
    }

    if lowered.starts_with("location set ") {
        return location_set(trimmed, invocation.state.clone()).await;
    }

    if lowered.starts_with("location reroll ") {
        return location_reroll(trimmed, invocation.state.clone()).await;
    }

    if lowered == "location save" {
        return location_save(invocation.state.clone()).await;
    }

    Ok(Some(ok_response("unknown location command. use `location help`".to_string(), None)))
}

async fn location_rename(trimmed: &str, state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    let name = trimmed[16..].trim();
    if name.is_empty() {
        return Ok(Some(ok_response("location name cannot be empty.".to_string(), None)));
    }

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.location_draft.clone()
    }.ok_or_else(|| "no active location draft. run create location or load <name>.".to_string())?;
    draft.name = name.to_string();

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Location;
        editor.location_draft = Some(draft.clone());
        editor.npc_draft = None;
    }

    Ok(Some(ok_response(location_summary_text(&draft), Some(location_event_from_draft(&draft)))))
}

async fn location_set(trimmed: &str, state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    let mut parts = trimmed.splitn(4, char::is_whitespace);
    let _ = parts.next();
    let _ = parts.next();
    let field = parts.next().unwrap_or_default();
    let value = parts.next().unwrap_or_default().trim();
    if value.is_empty() {
        return Ok(Some(ok_response("location set value cannot be empty.".to_string(), None)));
    }

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.location_draft.clone()
    }.ok_or_else(|| "no active location draft. run create location or load <name>.".to_string())?;

    let Some(canonical) =
        canonical_field_name(EntityKind::Location, field, FieldAccess::Set)
    else {
        let valid_fields = format_valid_field_list(EntityKind::Location, FieldAccess::Set);
        return Ok(Some(ok_response(
            format!("unknown location field: {}. valid fields: {}", field, valid_fields),
            None,
        )));
    };

    match canonical {
        "name" => draft.name = value.to_string(),
        "kind_type" => {
            draft.kind_type = normalize_location_kind_type(value)?;
            if draft.kind_type == "other" && draft.kind_custom.is_none() {
                draft.kind_custom = Some("Unknown".to_string());
            }
        }
        "kind_custom" => draft.kind_custom = Some(value.to_string()),
        "visual_description" => draft.visual_description = value.to_string(),
        "history_background" => draft.history_background = value.to_string(),
        "exports" => draft.exports = normalize_exports(parse_list_csv(value)),
        "tone" => draft.tone = value.to_string(),
        "authority" => draft.authority = value.to_string(),
        "danger_level" => draft.danger_level = normalize_location_danger_level(value)?,
        "current_tension" => draft.current_tension = value.to_string(),
        _ => {}
    }

    if draft.kind_type == "other" && draft.kind_custom.as_ref().is_none_or(|item| item.trim().is_empty()) {
        return Ok(Some(ok_response("kind_custom is required when kind is other. use location set kind_custom <value>.".to_string(), None)));
    }
    if draft.kind_type != "other" {
        draft.kind_custom = None;
    }

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Location;
        editor.location_draft = Some(draft.clone());
        editor.npc_draft = None;
    }

    Ok(Some(ok_response(location_summary_text(&draft), Some(location_event_from_draft(&draft)))))
}

async fn location_reroll(trimmed: &str, state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    if trimmed.eq_ignore_ascii_case("location reroll") {
        return Ok(Some(ok_response("usage: location reroll <field> [prompt]".to_string(), None)));
    }
    if trimmed.len() <= 16 {
        return Ok(Some(ok_response("usage: location reroll <field> [prompt]".to_string(), None)));
    }
    let args = trimmed[16..].trim();
    if args.is_empty() {
        return Ok(Some(ok_response("usage: location reroll <field> [prompt]".to_string(), None)));
    }
    let mut split = args.splitn(2, char::is_whitespace);
    let field = split.next().unwrap_or_default().trim().to_string();
    let prompt = normalize_optional_prompt(split.next().map(|value| value.to_string()));

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.location_draft.clone()
    }.ok_or_else(|| "no active location draft. run create location or load <name>.".to_string())?;

    let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

    let reroll_service = EntityRerollService;
    let workspace_root = state.workspace_root.clone();
    let database = state.database();
    let generation_repo = state.generation_repo();
    let rerolled = reroll_service
        .reroll_location_field(
            RerollLocationFieldInput {
                field,
                prompt,
                location: LocationRerollContext {
                    name: draft.name.clone(),
                    kind_type: draft.kind_type.clone(),
                    kind_custom: draft.kind_custom.clone(),
                    visual_description: draft.visual_description.clone(),
                    history_background: draft.history_background.clone(),
                    exports: draft.exports.clone(),
                    tone: draft.tone.clone(),
                    authority: draft.authority.clone(),
                    danger_level: draft.danger_level.clone(),
                    current_tension: draft.current_tension.clone(),
                },
            },
            &workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;

    match rerolled.field.as_str() {
        "name" => { if let Some(value) = rerolled.value { draft.name = value; } }
        "kind_type" => {
            if let Some(value) = rerolled.value {
                draft.kind_type = normalize_location_kind_type(&value)?;
                if draft.kind_type != "other" { draft.kind_custom = None; }
                else if draft.kind_custom.is_none() { draft.kind_custom = Some("Unknown".to_string()); }
            }
        }
        "kind_custom" => { if let Some(value) = rerolled.value { draft.kind_custom = Some(value); } }
        "visual_description" => { if let Some(value) = rerolled.value { draft.visual_description = value; } }
        "history_background" => { if let Some(value) = rerolled.value { draft.history_background = value; } }
        "exports" => { if let Some(exports) = rerolled.exports { draft.exports = exports; } }
        "tone" => { if let Some(value) = rerolled.value { draft.tone = value; } }
        "authority" => { if let Some(value) = rerolled.value { draft.authority = value; } }
        "danger_level" => { if let Some(value) = rerolled.value { draft.danger_level = normalize_location_danger_level(&value)?; } }
        "current_tension" => { if let Some(value) = rerolled.value { draft.current_tension = value; } }
        _ => {}
    }

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Location;
        editor.location_draft = Some(draft.clone());
        editor.npc_draft = None;
    }

    Ok(Some(ok_response(location_summary_text(&draft), Some(location_event_from_draft(&draft)))))
}

async fn location_save(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    let draft = {
        let editor = state.editor_session.lock().await;
        editor.location_draft.clone()
    }.ok_or_else(|| "no active location draft. run create location or load <name>.".to_string())?;

    let persistence = EntityPersistenceService;
    let result = persistence
        .save_location_draft(
        SaveLocationDraftInput {
            id: draft.id.clone(),
            name: draft.name.clone(),
            slug: draft.slug.clone(),
            vault_path: draft.vault_path.clone(),
            kind_type: draft.kind_type.clone(),
            kind_custom: draft.kind_custom.clone(),
            visual_description: draft.visual_description.clone(),
            history_background: draft.history_background.clone(),
            exports: draft.exports.clone(),
            tone: draft.tone.clone(),
            authority: draft.authority.clone(),
            danger_level: draft.danger_level.clone(),
            current_tension: draft.current_tension.clone(),
        },
            state.inner(),
        )
        .await?;

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::None;
        editor.npc_draft = None;
        editor.location_draft = None;
        editor.faction_draft = None;
    }

    let output = [
        "## Location saved".to_string(),
        format!("id: {}", result.id),
        format!("slug: {}", result.slug),
        format!("vault: {}", path_for_display(&result.vault_path)),
        format!("updated: {}", result.updated_at),
    ].join("\n");

    Ok(Some(ok_response(output, Some(CommandClientEvent::ClearDrafts))))
}

pub fn normalize_location_kind_type(value: &str) -> Result<String, String> {
    const LOCATION_KIND_TYPES: [&str; 10] = ["hamlet", "town", "city", "dungeon", "hideout", "ruin", "guildhall", "landmark", "wilderness", "other"];
    let normalized = value.trim().to_ascii_lowercase();
    if LOCATION_KIND_TYPES.contains(&normalized.as_str()) { Ok(normalized) }
    else { Err(format!("kind_type must be one of: {}", LOCATION_KIND_TYPES.join(", "))) }
}

pub fn normalize_location_danger_level(value: &str) -> Result<String, String> {
    const LOCATION_DANGER_LEVELS: [&str; 5] = ["Unknown", "safe", "guarded", "risky", "deadly"];
    let trimmed = value.trim();
    let normalized = if trimmed.eq_ignore_ascii_case("unknown") { "Unknown".to_string() } else { trimmed.to_ascii_lowercase() };
    if LOCATION_DANGER_LEVELS.contains(&normalized.as_str()) { Ok(normalized) }
    else { Err(format!("danger_level must be one of: {}", LOCATION_DANGER_LEVELS.join(", "))) }
}

pub fn parse_list_csv(value: &str) -> Vec<String> {
    value.split(',').map(|item| item.trim().to_string()).filter(|item| !item.is_empty()).collect()
}

pub fn normalize_exports(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values.into_iter().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).collect();
    if cleaned.is_empty() { vec!["Unknown".to_string()] } else { cleaned }
}

fn merge_seed_and_reroll_prompt(seed_prompt: &Option<String>, reroll_prompt: Option<String>) -> Option<String> {
    let seed_prompt = seed_prompt.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty());
    let reroll_prompt = reroll_prompt.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty());
    match (seed_prompt, reroll_prompt) {
        (Some(seed), Some(reroll)) => Some(format!("Seed context from original create command:\n{}\n\nReroll request:\n{}", seed, reroll)),
        (Some(seed), None) => Some(seed.to_string()),
        (None, Some(reroll)) => Some(reroll.to_string()),
        (None, None) => None,
    }
}

pub fn location_summary_text(draft: &LocationDraftSession) -> String {
    format!(
        "## Location Draft\nname: {}\nslug: {}\nkind: {}\nkind_custom: {}\nvisual: {}\nhistory: {}\nexports: {}\ntone: {}\nauthority: {}\ndanger: {}\ntension: {}\npath: {}",
        draft.name, draft.slug, draft.kind_type, draft.kind_custom.as_deref().unwrap_or("(none)"),
        draft.visual_description, draft.history_background, draft.exports.join(", "),
        draft.tone, draft.authority, draft.danger_level, draft.current_tension, draft.vault_path
    )
}

pub fn location_event_from_draft(draft: &LocationDraftSession) -> CommandClientEvent {
    use runebound_models::drafts::location_entity_card;
    use dnd_core::npc::normalize_unknown_text as core_normalize_unknown;
    use dnd_core::npc::normalize_unknown_list as core_normalize_list;

    let normalized_draft = LocationDraftSession {
        id: draft.id.clone(), name: draft.name.clone(), slug: draft.slug.clone(), vault_path: draft.vault_path.clone(),
        kind_type: draft.kind_type.clone(), kind_custom: draft.kind_custom.clone(),
        visual_description: core_normalize_unknown(&draft.visual_description),
        history_background: core_normalize_unknown(&draft.history_background),
        exports: core_normalize_list(draft.exports.clone()),
        tone: core_normalize_unknown(&draft.tone),
        authority: core_normalize_unknown(&draft.authority),
        danger_level: core_normalize_unknown(&draft.danger_level),
        current_tension: core_normalize_unknown(&draft.current_tension),
        seed_prompt: draft.seed_prompt.clone(),
    };
    let entity_card_doc = location_entity_card(&normalized_draft);
    CommandClientEvent::LoadLocationDraftWithCard { draft: normalized_draft, entity_card: entity_card_doc }
}
