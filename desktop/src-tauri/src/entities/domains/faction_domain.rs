use async_trait::async_trait;

use crate::app_state::{AppState, FactionDraftSession};
use crate::commands::ok_response;
use crate::entities::domain::{EntityDomain, EntityDomainResult};
use crate::entities::schema::{canonical_field_name, format_valid_field_list, FieldAccess, FACTION_SCHEMA};
use crate::entities::EntityKind;
use crate::services::entity_persistence::{EntityPersistenceService, SaveFactionDraftInput};
use crate::services::entity_reroll::{EntityRerollService, FactionRerollContext, RerollFactionFieldInput};
use crate::utils::{normalize_optional_prompt, path_for_display};
use dnd_core::command::CommandClientEvent;

pub struct FactionDomain;

impl FactionDomain {
    pub fn new() -> Self {
        Self
    }

    fn no_draft_response(&self) -> EntityDomainResult {
        Ok(Some(ok_response(
            "no active faction draft. run create faction or load <name>.".to_string(),
            None,
        )))
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

    async fn show_draft(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_faction().cloned()
        };
        let Some(draft) = draft else {
            return self.no_draft_response();
        };

        Ok(Some(ok_response(
            faction_summary_text(&draft),
            Some(faction_event_from_draft(&draft)),
        )))
    }

    async fn rename(&self, value: &str, state: &AppState) -> EntityDomainResult {
        let name = value.trim();
        if name.is_empty() {
            return Ok(Some(ok_response(
                "faction name cannot be empty.".to_string(),
                None,
            )));
        }

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor
                .get_faction_mut()
                .ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;
            draft.name = name.to_string();
            let snapshot = draft.clone();
            editor.activate(EntityKind::Faction);
            editor.clear_kind(EntityKind::Npc);
            editor.clear_kind(EntityKind::Location);
            snapshot
        };

        Ok(Some(ok_response(
            faction_summary_text(&updated),
            Some(faction_event_from_draft(&updated)),
        )))
    }

    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> EntityDomainResult {
        let trimmed_value = value.trim();
        if trimmed_value.is_empty() {
            return Ok(Some(ok_response(
                "faction set value cannot be empty.".to_string(),
                None,
            )));
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
            let draft = editor
                .get_faction_mut()
                .ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;

            match canonical {
                "name" => draft.name = trimmed_value.to_string(),
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
                "resources_assets" => draft.resources_assets = trimmed_value.to_string(),
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
                return Ok(Some(ok_response(
                    "kind_custom is required when kind is other. use faction set kind_custom <value>.".to_string(),
                    None,
                )));
            }
            if draft.kind_type != "other" {
                draft.kind_custom = None;
            }

            let snapshot = draft.clone();
            editor.activate(EntityKind::Faction);
            editor.clear_kind(EntityKind::Npc);
            editor.clear_kind(EntityKind::Location);
            snapshot
        };

        Ok(Some(ok_response(
            faction_summary_text(&updated),
            Some(faction_event_from_draft(&updated)),
        )))
    }

    async fn reroll_field(
        &self,
        field: &str,
        prompt: Option<String>,
        state: &AppState,
    ) -> EntityDomainResult {
        if field.trim().is_empty() {
            return Ok(Some(ok_response(
                "usage: faction reroll <field> [prompt]".to_string(),
                None,
            )));
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor
                .get_faction()
                .cloned()
        }.ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;

        let prompt = normalize_optional_prompt(prompt).map(|value| value.to_string());

        let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

        let reroll_service = EntityRerollService;
        let workspace_root = state.workspace_root.clone();
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
                &workspace_root,
                database.as_ref(),
                generation_repo.as_ref(),
            )
            .await?;

        match rerolled.field.as_str() {
            "name" => {
                if let Some(value) = rerolled.value {
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
                if let Some(value) = rerolled.value {
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
            editor.clear_kind(EntityKind::Npc);
            editor.clear_kind(EntityKind::Location);
        }

        Ok(Some(ok_response(
            faction_summary_text(&draft),
            Some(faction_event_from_draft(&draft)),
        )))
    }

    async fn save(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor
                .get_faction()
                .cloned()
        }.ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;

        let persistence = EntityPersistenceService;
        let result = persistence
            .save_faction_draft(
                SaveFactionDraftInput {
                    id: draft.id.clone(),
                    slug: draft.slug.clone(),
                    name: draft.name.clone(),
                    vault_path: draft.vault_path.clone(),
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
                state,
            )
            .await?;

        {
            let mut editor = state.editor_session.lock().await;
            editor.clear_all();
        }

        let output = [
            "## Faction saved".to_string(),
            format!("id: {}", result.id),
            format!("slug: {}", result.slug),
            format!("vault: {}", path_for_display(&result.vault_path)),
            format!("updated: {}", result.updated_at),
        ]
        .join("\n");

        Ok(Some(ok_response(
            output,
            Some(CommandClientEvent::ClearDrafts),
        )))
    }

    async fn cancel(&self, state: &AppState) -> EntityDomainResult {
        let removed = {
            let mut editor = state.editor_session.lock().await;
            editor.take_faction()
        };
        if removed.is_none() {
            return self.no_draft_response();
        }

        Ok(Some(ok_response(
            "faction draft discarded.".to_string(),
            Some(CommandClientEvent::ClearDrafts),
        )))
    }
}

fn merge_seed_and_reroll_prompt(
    seed_prompt: &Option<String>,
    reroll_prompt: Option<String>,
) -> Option<String> {
    let seed_prompt = seed_prompt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let reroll_prompt = reroll_prompt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    match (seed_prompt, reroll_prompt) {
        (Some(seed), Some(reroll)) => Some(format!(
            "Seed context from original create command:\n{}\n\nReroll request:\n{}",
            seed, reroll
        )),
        (Some(seed), None) => Some(seed.to_string()),
        (None, Some(reroll)) => Some(reroll.to_string()),
        (None, None) => None,
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

pub fn normalize_unknown_list(values: Vec<String>) -> Vec<String> {
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

pub fn parse_list_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
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
        draft.resources_assets,
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
    use dnd_core::npc::normalize_unknown_list as core_normalize_list;
    use dnd_core::npc::normalize_unknown_text as core_normalize_unknown;
    use runebound_models::drafts::faction_entity_card;

    let normalized_draft = FactionDraftSession {
        id: draft.id.clone(),
        name: draft.name.clone(),
        slug: draft.slug.clone(),
        vault_path: draft.vault_path.clone(),
        kind_type: draft.kind_type.clone(),
        kind_custom: draft.kind_custom.clone(),
        public_description: core_normalize_unknown(&draft.public_description),
        true_agenda: core_normalize_unknown(&draft.true_agenda),
        methods: core_normalize_unknown(&draft.methods),
        leadership: core_normalize_unknown(&draft.leadership),
        headquarters: core_normalize_unknown(&draft.headquarters),
        sphere_of_influence: core_normalize_unknown(&draft.sphere_of_influence),
        resources_assets: core_normalize_unknown(&draft.resources_assets),
        allies: core_normalize_list(draft.allies.clone()),
        rivals_enemies: core_normalize_list(draft.rivals_enemies.clone()),
        reputation: core_normalize_unknown(&draft.reputation),
        current_tension: core_normalize_unknown(&draft.current_tension),
        goals_short_term: core_normalize_list(draft.goals_short_term.clone()),
        goals_long_term: core_normalize_list(draft.goals_long_term.clone()),
        symbol_description: core_normalize_unknown(&draft.symbol_description),
        seed_prompt: draft.seed_prompt.clone(),
    };
    let entity_card_doc = faction_entity_card(&normalized_draft);
    CommandClientEvent::LoadFactionDraftWithCard {
        draft: normalized_draft,
        entity_card: entity_card_doc,
    }
}
