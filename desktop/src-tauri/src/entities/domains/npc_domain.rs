use async_trait::async_trait;

use crate::app_state::{AppState, DraftEnvelope, NpcDraftSession};
use crate::entities::EntityKind;
use crate::entities::common::{
    entity_message_response, entity_no_active_draft, entity_response_with_event,
    merge_seed_and_reroll_prompt, normalize_unknown_list, normalize_unknown_text,
};
use crate::entities::domain::{EntityDetail, EntityDomain, EntityDomainResult};
use crate::entities::schema::{
    FieldAccess, NPC_SCHEMA, canonical_field_name, format_valid_field_list,
};
use crate::services::entity_persistence::{EntityPersistenceService, SaveNpcDraftInput};
use crate::services::entity_reroll::{EntityRerollService, NpcRerollContext, RerollNpcFieldInput};
use crate::utils::{
    normalize_relative_path_for_storage, normalize_sex, parse_carrying_csv, path_for_display,
};
use dnd_core::command::CommandClientEvent;
use dnd_core::npc::slugify;
use dnd_core::serialization::carrying_from_db_text;

pub struct NpcDomain;

impl NpcDomain {
    pub fn new() -> Self {
        Self
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

    async fn resolve(
        &self,
        name_or_slug: &str,
        state: &AppState,
    ) -> Result<Option<EntityDetail>, String> {
        let database = state.database();
        let Some(row) = state
            .npc_repo()
            .find_by_name_or_slug(database.as_ref(), name_or_slug)
            .await?
        else {
            return Ok(None);
        };
        let draft = NpcDraftSession {
            id: row.id,
            seed_prompt: None,
            name: row.name,
            slug: row.slug,
            race: row.race,
            occupation: row.occupation,
            sex: normalize_sex(&row.sex).unwrap_or_else(|_| "male".to_string()),
            age: row.age,
            height: row.height,
            weight_lbs: row.weight_lbs,
            background: row.background,
            want_need: row.want_need,
            secret_obstacle: row.secret_obstacle,
            carrying: carrying_from_db_text(&row.carrying),
            location: row.location,
        };
        Ok(Some(EntityDetail {
            vault_path: normalize_relative_path_for_storage(&row.vault_path),
            created_at: Some(row.created_at),
            draft: DraftEnvelope::Npc(draft),
        }))
    }

    async fn show_draft(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_npc().cloned()
        };

        let Some(draft) = draft else {
            return entity_no_active_draft(EntityKind::Npc);
        };

        entity_response_with_event(npc_summary_text(&draft), npc_event_from_draft(&draft))
    }

    async fn rename(&self, value: &str, state: &AppState) -> EntityDomainResult {
        let name = value.trim();
        if name.is_empty() {
            return entity_message_response("npc name cannot be empty.");
        }

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor
                .get_npc_mut()
                .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;
            draft.name = name.to_string();
            draft.slug = slugify(name);
            draft.clone()
        };

        entity_response_with_event(npc_summary_text(&updated), npc_event_from_draft(&updated))
    }

    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> EntityDomainResult {
        let trimmed_value = value.trim();
        if trimmed_value.is_empty() {
            return entity_message_response("npc set value cannot be empty.");
        }

        let Some(canonical) = canonical_field_name(EntityKind::Npc, field, FieldAccess::Set) else {
            let valid_fields = format_valid_field_list(EntityKind::Npc, FieldAccess::Set);
            return entity_message_response(format!(
                "unknown npc field: {}. valid fields: {}",
                field, valid_fields
            ));
        };

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor
                .get_npc_mut()
                .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

            match canonical {
                "name" => {
                    draft.name = trimmed_value.to_string();
                    draft.slug = slugify(trimmed_value);
                }
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

            draft.clone()
        };

        entity_response_with_event(npc_summary_text(&updated), npc_event_from_draft(&updated))
    }

    async fn reroll_field(
        &self,
        field: &str,
        prompt: Option<String>,
        state: &AppState,
    ) -> EntityDomainResult {
        if field.trim().is_empty() {
            return entity_message_response("usage: npc reroll <field> [prompt]");
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.get_npc().cloned()
        }
        .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

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
                    draft.slug = slugify(&value);
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
        }

        entity_response_with_event(npc_summary_text(&draft), npc_event_from_draft(&draft))
    }

    async fn save(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_npc().cloned()
        }
        .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

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

        entity_response_with_event(output, CommandClientEvent::ClearDrafts)
    }

    async fn cancel(&self, state: &AppState) -> EntityDomainResult {
        let removed = {
            let mut editor = state.editor_session.lock().await;
            editor.take_npc()
        };

        if removed.is_none() {
            return entity_no_active_draft(EntityKind::Npc);
        }

        entity_response_with_event("npc draft discarded.", CommandClientEvent::ClearDrafts)
    }
}

pub fn npc_summary_text(draft: &NpcDraftSession) -> String {
    format!(
        "## NPC Draft\nname: {}\nslug: {}\nrace: {}\noccupation: {}\nsex: {}\nage: {}\nheight: {}\nweight: {}\nbackground: {}\nwant: {}\nsecret: {}\ncarrying: {}\nlocation: {}",
        draft.name,
        draft.slug,
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
    use runebound_models::drafts::npc_entity_card;

    let normalized_draft = NpcDraftSession {
        id: draft.id.clone(),
        name: draft.name.clone(),
        slug: draft.slug.clone(),
        race: normalize_unknown_text(&draft.race),
        occupation: normalize_unknown_text(&draft.occupation),
        sex: match draft.sex.to_lowercase().as_str() {
            "male" => "Male".to_string(),
            "female" => "Female".to_string(),
            _ => draft.sex.clone(),
        },
        age: normalize_unknown_text(&draft.age),
        height: normalize_unknown_text(&draft.height),
        weight_lbs: normalize_unknown_text(&draft.weight_lbs),
        background: normalize_unknown_text(&draft.background),
        want_need: normalize_unknown_text(&draft.want_need),
        secret_obstacle: normalize_unknown_text(&draft.secret_obstacle),
        carrying: normalize_unknown_list(draft.carrying.clone()),
        location: normalize_unknown_text(&draft.location),
        seed_prompt: draft.seed_prompt.clone(),
    };
    let entity_card_doc = npc_entity_card(&normalized_draft);
    CommandClientEvent::LoadNpcDraftWithCard {
        draft: normalized_draft,
        entity_card: entity_card_doc,
    }
}
