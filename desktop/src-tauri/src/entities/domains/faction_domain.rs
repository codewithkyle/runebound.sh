use async_trait::async_trait;

use crate::app_state::{AppState, DraftEnvelope, FactionDraftSession};
use crate::entities::EntityKind;
use crate::entities::common::{
    entity_message_response, entity_no_active_draft, entity_response_with_event,
    merge_seed_and_reroll_prompt, normalize_unknown_list, normalize_unknown_text, parse_list_csv,
};
use crate::entities::domain::{EntityDetail, EntityDomain, EntityDomainResult};
use crate::entities::schema::{
    FACTION_SCHEMA, FieldAccess, canonical_field_name, format_valid_field_list,
};
use crate::services::entity_reroll::{
    EntityRerollService, FactionRerollContext, RerollFactionFieldInput,
};
use crate::utils::{normalize_optional_prompt, path_for_display};
use dnd_core::command::CommandClientEvent;
use dnd_core::npc::slugify;
use dnd_core::serialization::faction_list_from_db_text;

pub struct FactionDomain;

impl FactionDomain {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EntityDomain for FactionDomain {
    fn kind(&self) -> EntityKind {
        EntityKind::Faction
    }

    fn schema(&self) -> &'static crate::entities::schema::EntitySchema {
        &FACTION_SCHEMA
    }

    fn help_text(&self) -> String {
        [
            "## Faction editor commands",
            "faction show",
            "faction rename <name>",
            "faction set <field> <value>",
            "faction reroll <field> [prompt]",
            "reroll",
            "faction save",
            "faction cancel",
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
            .faction_repo()
            .find_by_name_or_slug(database.as_ref(), name_or_slug)
            .await?
        else {
            return Ok(None);
        };
        let draft = FactionDraftSession {
            id: row.id,
            seed_prompt: None,
            name: row.name,
            slug: row.slug,
            vault_path: path_for_display(&row.vault_path),
            kind_type: row.kind_type,
            kind_custom: row.kind_custom,
            public_description: row.public_description,
            true_agenda: row.true_agenda,
            methods: row.methods,
            leadership: row.leadership,
            headquarters: row.headquarters,
            sphere_of_influence: row.sphere_of_influence,
            resources_assets: faction_list_from_db_text(&row.resources_assets),
            allies: faction_list_from_db_text(&row.allies),
            rivals_enemies: faction_list_from_db_text(&row.rivals_enemies),
            reputation: row.reputation,
            current_tension: row.current_tension,
            goals_short_term: faction_list_from_db_text(&row.goals_short_term),
            goals_long_term: faction_list_from_db_text(&row.goals_long_term),
            symbol_description: row.symbol_description,
        };
        Ok(Some(EntityDetail {
            draft: DraftEnvelope::Faction(draft),
        }))
    }

    async fn show_draft(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_faction().cloned()
        };
        let Some(draft) = draft else {
            return entity_no_active_draft(EntityKind::Faction);
        };

        entity_response_with_event(
            faction_summary_text(&draft),
            faction_event_from_draft(&draft),
        )
    }

    async fn rename(&self, value: &str, state: &AppState) -> EntityDomainResult {
        let name = value.trim();
        if name.is_empty() {
            return entity_message_response("faction name cannot be empty.");
        }

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor.get_faction_mut().ok_or_else(|| {
                "no active faction draft. run create faction or load <name>.".to_string()
            })?;
            draft.name = name.to_string();
            draft.slug = slugify(name);
            draft.clone()
        };

        entity_response_with_event(
            faction_summary_text(&updated),
            faction_event_from_draft(&updated),
        )
    }

    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> EntityDomainResult {
        let trimmed_value = value.trim();
        if trimmed_value.is_empty() {
            return entity_message_response("faction set value cannot be empty.");
        }

        let canonical = canonical_field_name(EntityKind::Faction, field, FieldAccess::Set)
            .ok_or_else(|| {
                let valid_fields = format_valid_field_list(EntityKind::Faction, FieldAccess::Set);
                format!(
                    "unknown faction reroll field: {}. valid fields: {}",
                    field, valid_fields
                )
            })?;

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor.get_faction_mut().ok_or_else(|| {
                "no active faction draft. run create faction or load <name>.".to_string()
            })?;

            match canonical {
                "name" => {
                    draft.name = trimmed_value.to_string();
                    draft.slug = slugify(trimmed_value);
                }
                "kind_type" => {
                    draft.kind_type = normalize_faction_kind_type(trimmed_value)?;
                    if draft.kind_type == "other" && draft.kind_custom.is_none() {
                        draft.kind_custom = Some("Unknown".to_string());
                    }
                }
                "kind_custom" => draft.kind_custom = Some(trimmed_value.to_string()),
                "public_description" => draft.public_description = trimmed_value.to_string(),
                "true_agenda" => draft.true_agenda = trimmed_value.to_string(),
                "methods" => draft.methods = trimmed_value.to_string(),
                "leadership" => draft.leadership = trimmed_value.to_string(),
                "headquarters" => draft.headquarters = trimmed_value.to_string(),
                "sphere_of_influence" => draft.sphere_of_influence = trimmed_value.to_string(),
                "resources_assets" => {
                    draft.resources_assets = normalize_unknown_list(parse_list_csv(trimmed_value));
                }
                "allies" => draft.allies = normalize_unknown_list(parse_list_csv(trimmed_value)),
                "rivals_enemies" => {
                    draft.rivals_enemies = normalize_unknown_list(parse_list_csv(trimmed_value));
                }
                "reputation" => draft.reputation = trimmed_value.to_string(),
                "current_tension" => draft.current_tension = trimmed_value.to_string(),
                "goals_short_term" => {
                    draft.goals_short_term = normalize_unknown_list(parse_list_csv(trimmed_value));
                }
                "goals_long_term" => {
                    draft.goals_long_term = normalize_unknown_list(parse_list_csv(trimmed_value));
                }
                "symbol_description" => draft.symbol_description = trimmed_value.to_string(),
                _ => {}
            }

            if draft.kind_type == "other"
                && draft
                    .kind_custom
                    .as_ref()
                    .is_none_or(|item| item.trim().is_empty())
            {
                return entity_message_response(
                    "kind_custom is required when kind is other. use faction set kind_custom <value>.",
                );
            }
            if draft.kind_type != "other" {
                draft.kind_custom = None;
            }

            draft.clone()
        };

        entity_response_with_event(
            faction_summary_text(&updated),
            faction_event_from_draft(&updated),
        )
    }

    async fn reroll_field(
        &self,
        field: &str,
        prompt: Option<String>,
        state: &AppState,
    ) -> EntityDomainResult {
        if field.trim().is_empty() {
            return entity_message_response("usage: faction reroll <field> [prompt]");
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.get_faction().cloned()
        }
        .ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;

        let prompt = normalize_optional_prompt(prompt).map(|value| value.to_string());

        let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

        let reroll_service = EntityRerollService;
        let database = state.database();
        let generation_repo = state.generation_repo();
        let rerolled = reroll_service
            .reroll_faction_field(
                RerollFactionFieldInput {
                    field: field.to_string(),
                    prompt,
                    faction: FactionRerollContext {
                        name: draft.name.clone(),
                        kind_type: draft.kind_type.clone(),
                        kind_custom: draft.kind_custom.clone(),
                        public_description: draft.public_description.clone(),
                        true_agenda: draft.true_agenda.clone(),
                        methods: draft.methods.clone(),
                        leadership: draft.leadership.clone(),
                        headquarters: draft.headquarters.clone(),
                        sphere_of_influence: draft.sphere_of_influence.clone(),
                        resources_assets: draft.resources_assets.clone(),
                        allies: draft.allies.clone(),
                        rivals_enemies: draft.rivals_enemies.clone(),
                        reputation: draft.reputation.clone(),
                        current_tension: draft.current_tension.clone(),
                        goals_short_term: draft.goals_short_term.clone(),
                        goals_long_term: draft.goals_long_term.clone(),
                        symbol_description: draft.symbol_description.clone(),
                    },
                },
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
                    draft.kind_type = normalize_faction_kind_type(&value)?;
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
            "public_description" => {
                if let Some(value) = rerolled.value {
                    draft.public_description = value;
                }
            }
            "true_agenda" => {
                if let Some(value) = rerolled.value {
                    draft.true_agenda = value;
                }
            }
            "methods" => {
                if let Some(value) = rerolled.value {
                    draft.methods = value;
                }
            }
            "leadership" => {
                if let Some(value) = rerolled.value {
                    draft.leadership = value;
                }
            }
            "headquarters" => {
                if let Some(value) = rerolled.value {
                    draft.headquarters = value;
                }
            }
            "sphere_of_influence" => {
                if let Some(value) = rerolled.value {
                    draft.sphere_of_influence = value;
                }
            }
            "resources_assets" => {
                if let Some(value) = rerolled.list_value {
                    draft.resources_assets = value;
                }
            }
            "allies" => {
                if let Some(value) = rerolled.list_value {
                    draft.allies = value;
                }
            }
            "rivals_enemies" => {
                if let Some(value) = rerolled.list_value {
                    draft.rivals_enemies = value;
                }
            }
            "reputation" => {
                if let Some(value) = rerolled.value {
                    draft.reputation = value;
                }
            }
            "current_tension" => {
                if let Some(value) = rerolled.value {
                    draft.current_tension = value;
                }
            }
            "goals_short_term" => {
                if let Some(value) = rerolled.list_value {
                    draft.goals_short_term = value;
                }
            }
            "goals_long_term" => {
                if let Some(value) = rerolled.list_value {
                    draft.goals_long_term = value;
                }
            }
            "symbol_description" => {
                if let Some(value) = rerolled.value {
                    draft.symbol_description = value;
                }
            }
            _ => {}
        }

        {
            let mut editor = state.editor_session.lock().await;
            editor.set_faction(draft.clone());
        }

        entity_response_with_event(
            faction_summary_text(&draft),
            faction_event_from_draft(&draft),
        )
    }

    async fn cancel(&self, state: &AppState) -> EntityDomainResult {
        let removed = {
            let mut editor = state.editor_session.lock().await;
            editor.take_faction()
        };
        if removed.is_none() {
            return entity_no_active_draft(EntityKind::Faction);
        }

        entity_response_with_event("faction draft discarded.", CommandClientEvent::ClearDrafts)
    }
}

pub fn normalize_faction_kind_type(value: &str) -> Result<String, String> {
    const FACTION_KIND_TYPES: [&str; 10] = [
        "guild",
        "cult",
        "military_order",
        "noble_house",
        "criminal_syndicate",
        "mercantile_league",
        "religious_order",
        "arcane_circle",
        "revolutionary_cell",
        "other",
    ];
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    if FACTION_KIND_TYPES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "kind_type must be one of: {}",
            FACTION_KIND_TYPES.join(", ")
        ))
    }
}

pub fn faction_summary_text(draft: &FactionDraftSession) -> String {
    format!(
        "## Faction Draft\nname: {}\nslug: {}\nkind: {}\nkind_custom: {}\npublic: {}\nagenda: {}\nmethods: {}\nleadership: {}\nheadquarters: {}\ninfluence: {}\nresources: {}\nallies: {}\nrivals: {}\nreputation: {}\ntension: {}\ngoals_short: {}\ngoals_long: {}\nsymbol: {}\npath: {}",
        draft.name,
        draft.slug,
        draft.kind_type,
        draft.kind_custom.as_deref().unwrap_or("(none)"),
        draft.public_description,
        draft.true_agenda,
        draft.methods,
        draft.leadership,
        draft.headquarters,
        draft.sphere_of_influence,
        draft.resources_assets.join(", "),
        draft.allies.join(", "),
        draft.rivals_enemies.join(", "),
        draft.reputation,
        draft.current_tension,
        draft.goals_short_term.join(", "),
        draft.goals_long_term.join(", "),
        draft.symbol_description,
        draft.vault_path,
    )
}

pub fn faction_event_from_draft(draft: &FactionDraftSession) -> CommandClientEvent {
    use runebound_models::drafts::faction_entity_card;

    let normalized_draft = FactionDraftSession {
        id: draft.id.clone(),
        name: draft.name.clone(),
        slug: draft.slug.clone(),
        vault_path: draft.vault_path.clone(),
        kind_type: draft.kind_type.clone(),
        kind_custom: draft.kind_custom.clone(),
        public_description: normalize_unknown_text(&draft.public_description),
        true_agenda: normalize_unknown_text(&draft.true_agenda),
        methods: normalize_unknown_text(&draft.methods),
        leadership: normalize_unknown_text(&draft.leadership),
        headquarters: normalize_unknown_text(&draft.headquarters),
        sphere_of_influence: normalize_unknown_text(&draft.sphere_of_influence),
        resources_assets: normalize_unknown_list(draft.resources_assets.clone()),
        allies: normalize_unknown_list(draft.allies.clone()),
        rivals_enemies: normalize_unknown_list(draft.rivals_enemies.clone()),
        reputation: normalize_unknown_text(&draft.reputation),
        current_tension: normalize_unknown_text(&draft.current_tension),
        goals_short_term: normalize_unknown_list(draft.goals_short_term.clone()),
        goals_long_term: normalize_unknown_list(draft.goals_long_term.clone()),
        symbol_description: normalize_unknown_text(&draft.symbol_description),
        seed_prompt: draft.seed_prompt.clone(),
    };
    let entity_card_doc = faction_entity_card(&normalized_draft);
    CommandClientEvent::LoadFactionDraftWithCard {
        draft: normalized_draft,
        entity_card: entity_card_doc,
    }
}
