use async_trait::async_trait;

use crate::app_state::{AppState, NpcDraftSession};
use crate::commands::ok_response;
use crate::entities::domain::{EntityDomain, EntityDomainResult};
use crate::entities::schema::{canonical_field_name, format_valid_field_list, FieldAccess, NPC_SCHEMA};
use crate::entities::EntityKind;
use crate::services::entity_persistence::{EntityPersistenceService, SaveNpcDraftInput};
use crate::services::entity_reroll::{EntityRerollService, NpcRerollContext, RerollNpcFieldInput};
use crate::utils::{normalize_sex, parse_carrying_csv, path_for_display};
use dnd_core::command::CommandClientEvent;

pub struct NpcDomain;

impl NpcDomain {
    pub fn new() -> Self {
        Self
    }

    fn no_draft_response(&self) -> EntityDomainResult {
        Ok(Some(ok_response(
            "no active npc draft. run create npc or load <name>.".to_string(),
            None,
        )))
    }
}

#[async_trait]
impl EntityDomain for NpcDomain {
    fn kind(&self) -> EntityKind {
        EntityKind::Npc
    }

    fn schema(&self) -> &'static crate::entities::schema::EntitySchema {
        &NPC_SCHEMA
    }

    fn help_text(&self) -> String {
        [
            "## NPC editor commands",
            "npc show",
            "npc rename <name>",
            "npc set <field> <value>",
            "npc travel to <location>",
            "npc reroll <field> [prompt]",
            "reroll",
            "npc save",
            "npc cancel",
        ]
        .join("\n")
    }

    async fn show_draft(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_npc().cloned()
        };

        let Some(draft) = draft else {
            return self.no_draft_response();
        };

        Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )))
    }

    async fn rename(&self, value: &str, state: &AppState) -> EntityDomainResult {
        let name = value.trim();
        if name.is_empty() {
            return Ok(Some(ok_response(
                "npc name cannot be empty.".to_string(),
                None,
            )));
        }

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor
                .get_npc_mut()
                .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;
            draft.name = name.to_string();
            let snapshot = draft.clone();
            editor.activate(EntityKind::Npc);
            editor.clear_kind(EntityKind::Location);
            snapshot
        };

        Ok(Some(ok_response(
            npc_summary_text(&updated),
            Some(npc_event_from_draft(&updated)),
        )))
    }

    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> EntityDomainResult {
        let trimmed_value = value.trim();
        if trimmed_value.is_empty() {
            return Ok(Some(ok_response(
                "npc set value cannot be empty.".to_string(),
                None,
            )));
        }

        let Some(canonical) = canonical_field_name(EntityKind::Npc, field, FieldAccess::Set) else {
            let valid_fields = format_valid_field_list(EntityKind::Npc, FieldAccess::Set);
            return Ok(Some(ok_response(
                format!("unknown npc field: {}. valid fields: {}", field, valid_fields),
                None,
            )));
        };

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor
                .get_npc_mut()
                .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

            match canonical {
                "name" => draft.name = trimmed_value.to_string(),
                "race" => draft.race = trimmed_value.to_string(),
                "occupation" => draft.occupation = trimmed_value.to_string(),
                "sex" => draft.sex = normalize_sex(trimmed_value)?,
                "age" => draft.age = trimmed_value.to_string(),
                "height" => draft.height = trimmed_value.to_string(),
                "weight_lbs" => draft.weight_lbs = trimmed_value.to_string(),
                "background" => draft.background = trimmed_value.to_string(),
                "want_need" => draft.want_need = trimmed_value.to_string(),
                "secret_obstacle" => draft.secret_obstacle = trimmed_value.to_string(),
                "carrying" => draft.carrying = parse_carrying_csv(trimmed_value),
                _ => {}
            }

            let snapshot = draft.clone();
            editor.activate(EntityKind::Npc);
            editor.clear_kind(EntityKind::Location);
            snapshot
        };

        Ok(Some(ok_response(
            npc_summary_text(&updated),
            Some(npc_event_from_draft(&updated)),
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
                "usage: npc reroll <field> [prompt]".to_string(),
                None,
            )));
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor
                .get_npc()
                .cloned()
        }.ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

        let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

        let reroll_service = EntityRerollService;
        let workspace_root = state.workspace_root.clone();
        let database = state.database();
        let generation_repo = state.generation_repo();
        let rerolled = reroll_service
            .reroll_npc_field(
                RerollNpcFieldInput {
                    field: field.to_string(),
                    prompt,
                    npc: NpcRerollContext {
                        name: draft.name.clone(),
                        race: draft.race.clone(),
                        occupation: draft.occupation.clone(),
                        sex: draft.sex.clone(),
                        age: draft.age.clone(),
                        height: draft.height.clone(),
                        weight_lbs: draft.weight_lbs.clone(),
                        background: draft.background.clone(),
                        want_need: draft.want_need.clone(),
                        secret_obstacle: draft.secret_obstacle.clone(),
                        carrying: draft.carrying.clone(),
                        location: draft.location.clone(),
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
            "race" => {
                if let Some(value) = rerolled.value {
                    draft.race = value;
                }
            }
            "occupation" => {
                if let Some(value) = rerolled.value {
                    draft.occupation = value;
                }
            }
            "sex" => {
                if let Some(value) = rerolled.value {
                    draft.sex = normalize_sex(&value)?;
                }
            }
            "age" => {
                if let Some(value) = rerolled.value {
                    draft.age = value;
                }
            }
            "height" => {
                if let Some(value) = rerolled.value {
                    draft.height = value;
                }
            }
            "weight_lbs" => {
                if let Some(value) = rerolled.value {
                    draft.weight_lbs = value;
                }
            }
            "background" => {
                if let Some(value) = rerolled.value {
                    draft.background = value;
                }
            }
            "want_need" => {
                if let Some(value) = rerolled.value {
                    draft.want_need = value;
                }
            }
            "secret_obstacle" => {
                if let Some(value) = rerolled.value {
                    draft.secret_obstacle = value;
                }
            }
            "carrying" => {
                if let Some(carrying) = rerolled.carrying {
                    draft.carrying = carrying;
                }
            }
            _ => {}
        }

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

    async fn save(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor
                .get_npc()
                .cloned()
        }.ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

        let persistence = EntityPersistenceService;
        let result = persistence
            .save_npc_draft(
                SaveNpcDraftInput {
                    id: draft.id.clone(),
                    name: draft.name.clone(),
                    race: draft.race.clone(),
                    occupation: draft.occupation.clone(),
                    sex: draft.sex.clone(),
                    age: draft.age.clone(),
                    height: draft.height.clone(),
                    weight_lbs: draft.weight_lbs.clone(),
                    background: draft.background.clone(),
                    want_need: draft.want_need.clone(),
                    secret_obstacle: draft.secret_obstacle.clone(),
                    carrying: draft.carrying.clone(),
                    location: draft.location.clone(),
                },
                state,
            )
            .await?;

        {
            let mut editor = state.editor_session.lock().await;
            editor.clear_all();
        }

        let output = [
            "## NPC saved".to_string(),
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
            editor.take_npc()
        };

        if removed.is_none() {
            return self.no_draft_response();
        }

        Ok(Some(ok_response(
            "npc draft discarded.".to_string(),
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

pub fn npc_summary_text(draft: &NpcDraftSession) -> String {
    format!(
        "## NPC Draft\nname: {}\nrace: {}\noccupation: {}\nsex: {}\nage: {}\nheight: {}\nweight: {}\nbackground: {}\nwant: {}\nsecret: {}\ncarrying: {}\nlocation: {}",
        draft.name,
        draft.race,
        draft.occupation,
        draft.sex,
        draft.age,
        draft.height,
        draft.weight_lbs,
        draft.background,
        draft.want_need,
        draft.secret_obstacle,
        draft.carrying.join(", "),
        draft.location,
    )
}

pub fn npc_event_from_draft(draft: &NpcDraftSession) -> CommandClientEvent {
    use dnd_core::npc::normalize_unknown_list as core_normalize_list;
    use dnd_core::npc::normalize_unknown_text as core_normalize_unknown;
    use runebound_models::drafts::npc_entity_card;

    let normalized_draft = NpcDraftSession {
        id: draft.id.clone(),
        name: draft.name.clone(),
        race: core_normalize_unknown(&draft.race),
        occupation: core_normalize_unknown(&draft.occupation),
        sex: match draft.sex.to_lowercase().as_str() {
            "male" => "Male".to_string(),
            "female" => "Female".to_string(),
            _ => draft.sex.clone(),
        },
        age: core_normalize_unknown(&draft.age),
        height: core_normalize_unknown(&draft.height),
        weight_lbs: core_normalize_unknown(&draft.weight_lbs),
        background: core_normalize_unknown(&draft.background),
        want_need: core_normalize_unknown(&draft.want_need),
        secret_obstacle: core_normalize_unknown(&draft.secret_obstacle),
        carrying: core_normalize_list(draft.carrying.clone()),
        location: core_normalize_unknown(&draft.location),
        seed_prompt: draft.seed_prompt.clone(),
    };
    let entity_card_doc = npc_entity_card(&normalized_draft);
    CommandClientEvent::LoadNpcDraftWithCard {
        draft: normalized_draft,
        entity_card: entity_card_doc,
    }
}
