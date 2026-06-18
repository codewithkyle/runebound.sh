use async_trait::async_trait;

use crate::app_state::{AppState, DraftEnvelope, LocationDraftSession};
use crate::entities::EntityKind;
use crate::entities::common::{
    entity_message_response, entity_no_active_draft, entity_response_with_event,
    merge_seed_and_reroll_prompt, normalize_unknown_list, normalize_unknown_text, parse_list_csv,
};
use crate::entities::domain::{EntityDetail, EntityDomain, EntityDomainResult};
use crate::entities::schema::{
    FieldAccess, LOCATION_SCHEMA, canonical_field_name, format_valid_field_list,
};
use crate::services::entity_reroll::{
    EntityRerollService, LocationRerollContext, RerollLocationFieldInput,
};
use crate::utils::{normalize_relative_path_for_storage, path_for_display};
use dnd_core::command::CommandClientEvent;
use dnd_core::npc::slugify;
use dnd_core::serialization::exports_from_db_text;

pub struct LocationDomain;

impl LocationDomain {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EntityDomain for LocationDomain {
    fn kind(&self) -> EntityKind {
        EntityKind::Location
    }

    fn schema(&self) -> &'static crate::entities::schema::EntitySchema {
        &LOCATION_SCHEMA
    }

    fn help_text(&self) -> String {
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
        .join("\n")
    }

    async fn resolve(
        &self,
        name_or_slug: &str,
        state: &AppState,
    ) -> Result<Option<EntityDetail>, String> {
        let database = state.database();
        let Some(row) = state
            .location_repo()
            .find_by_name_or_slug(database.as_ref(), name_or_slug)
            .await?
        else {
            return Ok(None);
        };
        let draft = LocationDraftSession {
            id: row.id,
            seed_prompt: None,
            name: row.name,
            slug: row.slug,
            vault_path: path_for_display(&row.vault_path),
            kind_type: row.kind_type,
            kind_custom: row.kind_custom,
            visual_description: row.visual_description,
            history_background: row.history_background,
            exports: exports_from_db_text(&row.exports),
            tone: row.tone,
            authority: row.authority,
            danger_level: row.danger_level,
            current_tension: row.current_tension,
        };
        Ok(Some(EntityDetail {
            vault_path: normalize_relative_path_for_storage(&row.vault_path),
            created_at: Some(row.created_at),
            draft: DraftEnvelope::Location(draft),
        }))
    }

    async fn show_draft(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_location().cloned()
        };
        let Some(draft) = draft else {
            return entity_no_active_draft(EntityKind::Location);
        };

        entity_response_with_event(
            location_summary_text(&draft),
            location_event_from_draft(&draft),
        )
    }

    async fn rename(&self, value: &str, state: &AppState) -> EntityDomainResult {
        let name = value.trim();
        if name.is_empty() {
            return entity_message_response("location name cannot be empty.");
        }

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor.get_location_mut().ok_or_else(|| {
                "no active location draft. run create location or load <name>.".to_string()
            })?;
            draft.name = name.to_string();
            draft.slug = slugify(name);
            draft.clone()
        };

        entity_response_with_event(
            location_summary_text(&updated),
            location_event_from_draft(&updated),
        )
    }

    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> EntityDomainResult {
        let trimmed_value = value.trim();
        if trimmed_value.is_empty() {
            return entity_message_response("location set value cannot be empty.");
        }

        let Some(canonical) = canonical_field_name(EntityKind::Location, field, FieldAccess::Set)
        else {
            let valid_fields = format_valid_field_list(EntityKind::Location, FieldAccess::Set);
            return entity_message_response(format!(
                "unknown location field: {}. valid fields: {}",
                field, valid_fields
            ));
        };

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor.get_location_mut().ok_or_else(|| {
                "no active location draft. run create location or load <name>.".to_string()
            })?;

            match canonical {
                "name" => {
                    draft.name = trimmed_value.to_string();
                    draft.slug = slugify(trimmed_value);
                }
                "kind_type" => {
                    draft.kind_type = normalize_location_kind_type(trimmed_value)?;
                    if draft.kind_type == "other" && draft.kind_custom.is_none() {
                        draft.kind_custom = Some("Unknown".to_string());
                    }
                }
                "kind_custom" => draft.kind_custom = Some(trimmed_value.to_string()),
                "visual_description" => draft.visual_description = trimmed_value.to_string(),
                "history_background" => draft.history_background = trimmed_value.to_string(),
                "exports" => draft.exports = normalize_exports(parse_list_csv(trimmed_value)),
                "tone" => draft.tone = trimmed_value.to_string(),
                "authority" => draft.authority = trimmed_value.to_string(),
                "danger_level" => {
                    draft.danger_level = normalize_location_danger_level(trimmed_value)?;
                }
                "current_tension" => draft.current_tension = trimmed_value.to_string(),
                _ => {}
            }

            if draft.kind_type == "other"
                && draft
                    .kind_custom
                    .as_ref()
                    .is_none_or(|item| item.trim().is_empty())
            {
                return entity_message_response(
                    "kind_custom is required when kind is other. use location set kind_custom <value>.",
                );
            }
            if draft.kind_type != "other" {
                draft.kind_custom = None;
            }

            draft.clone()
        };

        entity_response_with_event(
            location_summary_text(&updated),
            location_event_from_draft(&updated),
        )
    }

    async fn reroll_field(
        &self,
        field: &str,
        prompt: Option<String>,
        state: &AppState,
    ) -> EntityDomainResult {
        if field.trim().is_empty() {
            return entity_message_response("usage: location reroll <field> [prompt]");
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.get_location().cloned()
        }
        .ok_or_else(|| {
            "no active location draft. run create location or load <name>.".to_string()
        })?;

        let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

        let reroll_service = EntityRerollService;
        let workspace_root = state.workspace_root.clone();
        let database = state.database();
        let generation_repo = state.generation_repo();
        let rerolled = reroll_service
            .reroll_location_field(
                RerollLocationFieldInput {
                    field: field.to_string(),
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
            "name" => {
                if let Some(value) = rerolled.value {
                    draft.slug = slugify(&value);
                    draft.name = value;
                }
            }
            "kind_type" => {
                if let Some(value) = rerolled.value {
                    draft.kind_type = normalize_location_kind_type(&value)?;
                    if draft.kind_type != "other" {
                        draft.kind_custom = None;
                    } else if draft.kind_custom.is_none() {
                        draft.kind_custom = Some("Unknown".to_string());
                    }
                }
            }
            "kind_custom" => {
                if let Some(value) = rerolled.value {
                    draft.kind_custom = Some(value);
                }
            }
            "visual_description" => {
                if let Some(value) = rerolled.value {
                    draft.visual_description = value;
                }
            }
            "history_background" => {
                if let Some(value) = rerolled.value {
                    draft.history_background = value;
                }
            }
            "exports" => {
                if let Some(exports) = rerolled.exports {
                    draft.exports = exports;
                }
            }
            "tone" => {
                if let Some(value) = rerolled.value {
                    draft.tone = value;
                }
            }
            "authority" => {
                if let Some(value) = rerolled.value {
                    draft.authority = value;
                }
            }
            "danger_level" => {
                if let Some(value) = rerolled.value {
                    draft.danger_level = normalize_location_danger_level(&value)?;
                }
            }
            "current_tension" => {
                if let Some(value) = rerolled.value {
                    draft.current_tension = value;
                }
            }
            _ => {}
        }

        {
            let mut editor = state.editor_session.lock().await;
            editor.set_location(draft.clone());
        }

        entity_response_with_event(
            location_summary_text(&draft),
            location_event_from_draft(&draft),
        )
    }

    async fn cancel(&self, state: &AppState) -> EntityDomainResult {
        let removed = {
            let mut editor = state.editor_session.lock().await;
            editor.take_location()
        };
        if removed.is_none() {
            return entity_no_active_draft(EntityKind::Location);
        }

        entity_response_with_event("location draft discarded.", CommandClientEvent::ClearDrafts)
    }
}

pub fn normalize_location_kind_type(value: &str) -> Result<String, String> {
    const LOCATION_KIND_TYPES: [&str; 10] = [
        "hamlet",
        "town",
        "city",
        "dungeon",
        "hideout",
        "ruin",
        "guildhall",
        "landmark",
        "wilderness",
        "other",
    ];
    let normalized = value.trim().to_ascii_lowercase();
    if LOCATION_KIND_TYPES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "kind_type must be one of: {}",
            LOCATION_KIND_TYPES.join(", ")
        ))
    }
}

pub fn normalize_location_danger_level(value: &str) -> Result<String, String> {
    const LOCATION_DANGER_LEVELS: [&str; 5] = ["Unknown", "safe", "guarded", "risky", "deadly"];
    let trimmed = value.trim();
    let normalized = if trimmed.eq_ignore_ascii_case("unknown") {
        "Unknown".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    };
    if LOCATION_DANGER_LEVELS.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "danger_level must be one of: {}",
            LOCATION_DANGER_LEVELS.join(", ")
        ))
    }
}

pub fn normalize_exports(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if cleaned.is_empty() {
        vec!["Unknown".to_string()]
    } else {
        cleaned
    }
}

pub fn location_summary_text(draft: &LocationDraftSession) -> String {
    format!(
        "## Location Draft\nname: {}\nslug: {}\nkind: {}\nkind_custom: {}\nvisual: {}\nhistory: {}\nexports: {}\ntone: {}\nauthority: {}\ndanger: {}\ntension: {}\npath: {}",
        draft.name,
        draft.slug,
        draft.kind_type,
        draft.kind_custom.as_deref().unwrap_or("(none)"),
        draft.visual_description,
        draft.history_background,
        draft.exports.join(", "),
        draft.tone,
        draft.authority,
        draft.danger_level,
        draft.current_tension,
        draft.vault_path,
    )
}

pub fn location_event_from_draft(draft: &LocationDraftSession) -> CommandClientEvent {
    use runebound_models::drafts::location_entity_card;

    let normalized_draft = LocationDraftSession {
        id: draft.id.clone(),
        name: draft.name.clone(),
        slug: draft.slug.clone(),
        vault_path: draft.vault_path.clone(),
        kind_type: draft.kind_type.clone(),
        kind_custom: draft.kind_custom.clone(),
        visual_description: normalize_unknown_text(&draft.visual_description),
        history_background: normalize_unknown_text(&draft.history_background),
        exports: normalize_unknown_list(draft.exports.clone()),
        tone: normalize_unknown_text(&draft.tone),
        authority: normalize_unknown_text(&draft.authority),
        danger_level: normalize_unknown_text(&draft.danger_level),
        current_tension: normalize_unknown_text(&draft.current_tension),
        seed_prompt: draft.seed_prompt.clone(),
    };
    let entity_card_doc = location_entity_card(&normalized_draft);
    CommandClientEvent::LoadLocationDraftWithCard {
        draft: normalized_draft,
        entity_card: entity_card_doc,
    }
}
