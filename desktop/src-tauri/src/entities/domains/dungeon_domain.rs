use async_trait::async_trait;

use crate::app_state::{AppState, DraftEnvelope, DungeonDraftSession};
use crate::entities::EntityKind;
use crate::entities::common::{
    entity_message_response, entity_no_active_draft, entity_response_with_event,
    merge_seed_and_reroll_prompt,
};
use crate::entities::domain::{EntityDetail, EntityDomain, EntityDomainResult};
use crate::entities::schema::{
    DUNGEON_SCHEMA, FieldAccess, canonical_field_name, format_valid_field_list,
};
use crate::services::entity_persistence::{EntityPersistenceService, SaveDungeonDraftInput};
use crate::services::entity_reroll::{
    DungeonRerollContext, EntityRerollService, RerollDungeonBeatInput, RerollDungeonFieldInput,
};
use crate::utils::{
    normalize_dungeon_tone, normalize_dungeon_topology, normalize_dungeon_twist,
    normalize_optional_prompt, normalize_relative_path_for_storage, normalize_unknown_text,
    path_for_display,
};
use dnd_core::command::CommandClientEvent;
use dnd_core::npc::slugify;
use runebound_models::utils::{DUNGEON_FUNCTIONS, normalize_dungeon_content_type};

pub struct DungeonDomain;

impl DungeonDomain {
    pub fn new() -> Self {
        Self
    }
}

/// Resolve a `<beat>` token (a function name like `setback`, or `1`–`5`) to its
/// 0-based index in the fixed five-beat skeleton.
pub fn beat_index_from_token(token: &str) -> Option<usize> {
    let trimmed = token.trim();
    if let Ok(n) = trimmed.parse::<usize>() {
        if (1..=DUNGEON_FUNCTIONS.len()).contains(&n) {
            return Some(n - 1);
        }
        return None;
    }
    DUNGEON_FUNCTIONS
        .iter()
        .position(|func| func.eq_ignore_ascii_case(trimmed))
}

const BEAT_FIELDS: [&str; 6] = [
    "content_type",
    "idea",
    "player_goals",
    "lever",
    "loot",
    "design_note",
];

fn canonical_beat_field(raw: &str) -> Option<&'static str> {
    let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
    match normalized.as_str() {
        "content_type" | "type" | "content" => Some("content_type"),
        "idea" => Some("idea"),
        "player_goals" | "playergoals" | "goals" | "goal" => Some("player_goals"),
        "lever" | "hook" => Some("lever"),
        "loot" | "reward" => Some("loot"),
        "design_note" | "designnote" | "design" | "note" => Some("design_note"),
        _ => None,
    }
}

#[async_trait]
impl EntityDomain for DungeonDomain {
    fn kind(&self) -> EntityKind {
        EntityKind::Dungeon
    }

    fn schema(&self) -> &'static crate::entities::schema::EntitySchema {
        &DUNGEON_SCHEMA
    }

    fn help_text(&self) -> String {
        [
            "## Dungeon editor commands",
            "dungeon show",
            "dungeon rename <name>",
            "dungeon set <field> <value>",
            "dungeon set <beat> <field> <value>",
            "dungeon reroll <beat>",
            "dungeon reroll premise",
            "reroll <beat>      (reroll just that card)",
            "reroll             (reroll the whole dungeon)",
            "dungeon save",
            "dungeon cancel",
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
            .dungeon_repo()
            .find_by_name_or_slug(database.as_ref(), name_or_slug)
            .await?
        else {
            return Ok(None);
        };
        let draft = DungeonDraftSession {
            id: row.id,
            seed_prompt: None,
            name: row.name,
            slug: row.slug,
            vault_path: path_for_display(&row.vault_path),
            location: row.location,
            story: row.story,
            premise: row.premise,
            topology: row.topology,
            tone: row.tone,
            twist: row.twist,
            beats: serde_json::from_str(&row.beats_json).unwrap_or_default(),
        };
        Ok(Some(EntityDetail {
            vault_path: normalize_relative_path_for_storage(&row.vault_path),
            created_at: Some(row.created_at),
            draft: DraftEnvelope::Dungeon(draft),
        }))
    }

    async fn show_draft(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_dungeon().cloned()
        };
        let Some(draft) = draft else {
            return entity_no_active_draft(EntityKind::Dungeon);
        };

        entity_response_with_event(
            dungeon_summary_text(&draft),
            dungeon_event_from_draft(&draft),
        )
    }

    async fn rename(&self, value: &str, state: &AppState) -> EntityDomainResult {
        let name = value.trim();
        if name.is_empty() {
            return entity_message_response("dungeon name cannot be empty.");
        }

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor.get_dungeon_mut().ok_or_else(|| {
                "no active dungeon draft. run create dungeon or load <name>.".to_string()
            })?;
            draft.name = name.to_string();
            draft.slug = slugify(name);
            draft.clone()
        };

        entity_response_with_event(
            dungeon_summary_text(&updated),
            dungeon_event_from_draft(&updated),
        )
    }

    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> EntityDomainResult {
        let trimmed_value = value.trim();

        // Beat-level edit: `dungeon set <beat> <field> <value>`. The router passes
        // `field = <beat>` and `value = "<field> <value>"`, so a beat token here
        // means we re-split `value` into the beat field and its value.
        if let Some(beat_index) = beat_index_from_token(field) {
            let mut parts = trimmed_value.splitn(2, char::is_whitespace);
            let beat_field_raw = parts.next().unwrap_or_default();
            let beat_value = parts.next().unwrap_or_default().trim();
            let beat_field = canonical_beat_field(beat_field_raw).ok_or_else(|| {
                format!(
                    "unknown dungeon beat field: {}. valid fields: {}",
                    beat_field_raw,
                    BEAT_FIELDS.join(", ")
                )
            })?;
            if beat_field != "loot" && beat_value.is_empty() {
                return entity_message_response("dungeon set value cannot be empty.");
            }

            let updated = {
                let mut editor = state.editor_session.lock().await;
                let draft = editor.get_dungeon_mut().ok_or_else(|| {
                    "no active dungeon draft. run create dungeon or load <name>.".to_string()
                })?;
                let beat = draft.beats.get_mut(beat_index).ok_or_else(|| {
                    format!("beat {} is not present on this dungeon.", beat_index + 1)
                })?;
                match beat_field {
                    "content_type" => {
                        beat.content_type = normalize_dungeon_content_type(beat_value)?
                    }
                    "idea" => beat.idea = beat_value.to_string(),
                    "lever" => beat.lever = beat_value.to_string(),
                    "loot" => {
                        // "none"/empty clears the conditional loot line.
                        beat.loot =
                            if beat_value.is_empty() || beat_value.eq_ignore_ascii_case("none") {
                                None
                            } else {
                                Some(beat_value.to_string())
                            };
                    }
                    "player_goals" => beat.player_goals = beat_value.to_string(),
                    "design_note" => beat.design_note = beat_value.to_string(),
                    _ => {}
                }
                draft.clone()
            };

            return entity_response_with_event(
                dungeon_summary_text(&updated),
                dungeon_event_from_draft(&updated),
            );
        }

        // Dungeon-level scalar field.
        if trimmed_value.is_empty() {
            return entity_message_response("dungeon set value cannot be empty.");
        }
        let canonical = canonical_field_name(EntityKind::Dungeon, field, FieldAccess::Set)
            .ok_or_else(|| {
                let valid_fields = format_valid_field_list(EntityKind::Dungeon, FieldAccess::Set);
                format!(
                    "unknown dungeon set field: {}. valid fields: {}",
                    field, valid_fields
                )
            })?;

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor.get_dungeon_mut().ok_or_else(|| {
                "no active dungeon draft. run create dungeon or load <name>.".to_string()
            })?;

            match canonical {
                "name" => {
                    draft.name = trimmed_value.to_string();
                    draft.slug = slugify(trimmed_value);
                }
                "location" => draft.location = trimmed_value.to_string(),
                "premise" => draft.premise = trimmed_value.to_string(),
                "topology" => draft.topology = normalize_dungeon_topology(trimmed_value)?,
                "tone" => draft.tone = normalize_dungeon_tone(trimmed_value)?,
                "twist" => draft.twist = normalize_dungeon_twist(trimmed_value)?,
                _ => {}
            }

            draft.clone()
        };

        entity_response_with_event(
            dungeon_summary_text(&updated),
            dungeon_event_from_draft(&updated),
        )
    }

    async fn reroll_field(
        &self,
        field: &str,
        prompt: Option<String>,
        state: &AppState,
    ) -> EntityDomainResult {
        if field.trim().is_empty() {
            return entity_message_response("usage: dungeon reroll <beat>|premise|name [prompt]");
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.get_dungeon().cloned()
        }
        .ok_or_else(|| "no active dungeon draft. run create dungeon or load <name>.".to_string())?;

        let prompt = normalize_optional_prompt(prompt).map(|value| value.to_string());
        let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

        let reroll_service = EntityRerollService;
        let workspace_root = state.workspace_root.clone();
        let database = state.database();
        let generation_repo = state.generation_repo();

        // Per-beat reroll: `<beat>` resolves to one of the five fixed beats.
        if let Some(beat_index) = beat_index_from_token(field) {
            if draft.beats.len() != DUNGEON_FUNCTIONS.len() {
                return Err(
                    "dungeon does not have its five beats; reroll the whole dungeon first."
                        .to_string(),
                );
            }
            let rerolled = reroll_service
                .reroll_dungeon_beat(
                    RerollDungeonBeatInput {
                        beat_index,
                        prompt,
                        dungeon: DungeonRerollContext::from_draft(&draft),
                    },
                    &workspace_root,
                    database.as_ref(),
                    generation_repo.as_ref(),
                )
                .await?;
            // Write back only this beat; the other four stay byte-identical.
            draft.beats[beat_index] = rerolled.beat;

            {
                let mut editor = state.editor_session.lock().await;
                editor.set_dungeon(draft.clone());
            }
            return entity_response_with_event(
                dungeon_summary_text(&draft),
                dungeon_event_from_draft(&draft),
            );
        }

        // Dungeon-level scalar reroll (premise / name).
        let canonical = canonical_field_name(EntityKind::Dungeon, field, FieldAccess::Reroll)
            .ok_or_else(|| {
                let valid = format_valid_field_list(EntityKind::Dungeon, FieldAccess::Reroll);
                format!(
                    "cannot reroll dungeon field: {}. rerollable fields: {} (or a beat name).",
                    field, valid
                )
            })?;

        let rerolled = reroll_service
            .reroll_dungeon_field(
                RerollDungeonFieldInput {
                    field: canonical.to_string(),
                    prompt,
                    dungeon: DungeonRerollContext::from_draft(&draft),
                },
                &workspace_root,
                database.as_ref(),
                generation_repo.as_ref(),
            )
            .await?;

        if let Some(value) = rerolled.value {
            match canonical {
                "name" => {
                    draft.slug = slugify(&value);
                    draft.name = value;
                }
                "location" => draft.location = value,
                "premise" => draft.premise = value,
                _ => {}
            }
        }

        {
            let mut editor = state.editor_session.lock().await;
            editor.set_dungeon(draft.clone());
        }

        entity_response_with_event(
            dungeon_summary_text(&draft),
            dungeon_event_from_draft(&draft),
        )
    }

    async fn save(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_dungeon().cloned()
        }
        .ok_or_else(|| "no active dungeon draft. run create dungeon or load <name>.".to_string())?;

        let persistence = EntityPersistenceService;
        let result = persistence
            .save_dungeon_draft(
                SaveDungeonDraftInput {
                    id: draft.id.clone(),
                    name: draft.name.clone(),
                    vault_path: draft.vault_path.clone(),
                    location: draft.location.clone(),
                    story: draft.story.clone(),
                    premise: draft.premise.clone(),
                    topology: draft.topology.clone(),
                    tone: draft.tone.clone(),
                    twist: draft.twist.clone(),
                    beats: draft.beats.clone(),
                },
                state,
            )
            .await?;

        {
            let mut editor = state.editor_session.lock().await;
            editor.clear_all();
        }

        let output = [
            "## Dungeon saved".to_string(),
            format!("id: {}", result.id),
            format!("slug: {}", result.slug),
            format!("vault: {}", path_for_display(&result.vault_path)),
            format!("updated: {}", result.updated_at),
        ]
        .join("\n");

        entity_response_with_event(output, CommandClientEvent::ClearDrafts)
    }

    async fn cancel(&self, state: &AppState) -> EntityDomainResult {
        let removed = {
            let mut editor = state.editor_session.lock().await;
            editor.take_dungeon()
        };
        if removed.is_none() {
            return entity_no_active_draft(EntityKind::Dungeon);
        }

        entity_response_with_event("dungeon draft discarded.", CommandClientEvent::ClearDrafts)
    }
}

pub fn dungeon_summary_text(draft: &DungeonDraftSession) -> String {
    let mut lines = vec![
        "## Dungeon Draft".to_string(),
        format!("name: {}", draft.name),
        format!("slug: {}", draft.slug),
        format!("location: {}", draft.location),
        format!("premise: {}", draft.premise),
        format!("topology: {}", draft.topology),
        format!("tone: {}", draft.tone),
        format!("twist: {}", draft.twist),
    ];
    for (i, beat) in draft.beats.iter().enumerate() {
        lines.push(format!(
            "beat {} [{}] {}: idea={} | player_goals={} | lever={} | loot={} | design={}",
            i + 1,
            beat.content_type,
            beat.function,
            beat.idea,
            beat.player_goals,
            beat.lever,
            beat.loot.as_deref().unwrap_or("none"),
            beat.design_note,
        ));
    }
    lines.push(format!("path: {}", draft.vault_path));
    lines.join("\n")
}

pub fn dungeon_event_from_draft(draft: &DungeonDraftSession) -> CommandClientEvent {
    use runebound_models::drafts::dungeon_entity_card;

    let mut normalized_draft = draft.clone();
    normalized_draft.location = normalize_unknown_text(&draft.location);
    normalized_draft.premise = normalize_unknown_text(&draft.premise);
    for beat in normalized_draft.beats.iter_mut() {
        beat.content_type = normalize_unknown_text(&beat.content_type);
        beat.idea = normalize_unknown_text(&beat.idea);
        beat.player_goals = normalize_unknown_text(&beat.player_goals);
        beat.lever = normalize_unknown_text(&beat.lever);
        beat.design_note = normalize_unknown_text(&beat.design_note);
        beat.loot = beat
            .loot
            .as_ref()
            .map(|loot| loot.trim().to_string())
            .filter(|loot| !loot.is_empty());
    }
    let entity_card_doc = dungeon_entity_card(&normalized_draft);
    CommandClientEvent::LoadDungeonDraftWithCard {
        draft: normalized_draft,
        entity_card: entity_card_doc,
    }
}
