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
use crate::utils::{
    normalize_faction_kind_type, normalize_loyalty_type, normalize_optional_prompt,
    path_for_display,
};
use dnd_core::command::CommandClientEvent;
use dnd_core::npc::slugify;
use dnd_core::serialization::{faction_link_list_from_db_text, faction_list_from_db_text};

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
            public_description: row.public_description,
            reputation: row.reputation,
            symbol_description: row.symbol_description,
            want: row.want,
            obstacle: row.obstacle,
            action: row.action,
            consequence: row.consequence,
            leader: row.leader,
            sphere_of_influence: row.sphere_of_influence,
            resources_assets: faction_list_from_db_text(&row.resources_assets),
            // Relational lists preserve blank (D4): never coerce an empty list to
            // ["Unknown"] the way LLM-content `resources_assets` does.
            allies: faction_link_list_from_db_text(&row.allies),
            rivals_enemies: faction_link_list_from_db_text(&row.rivals_enemies),
            liege: row.liege,
            loyalty_type: row.loyalty_type,
            // `category` (row column, D2) is derived from kind, not stored on the
            // draft. Loading an existing row never re-subfolders it (mirrors
            // location), so `wizard_subfoldered` is always false here.
            wizard_subfoldered: false,
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
                "kind_type" => draft.kind_type = normalize_faction_kind_type(trimmed_value)?,
                "public_description" => draft.public_description = trimmed_value.to_string(),
                "reputation" => draft.reputation = trimmed_value.to_string(),
                "symbol_description" => draft.symbol_description = trimmed_value.to_string(),
                "want" => draft.want = trimmed_value.to_string(),
                "obstacle" => draft.obstacle = trimmed_value.to_string(),
                "action" => draft.action = trimmed_value.to_string(),
                "consequence" => draft.consequence = trimmed_value.to_string(),
                "sphere_of_influence" => draft.sphere_of_influence = trimmed_value.to_string(),
                "resources_assets" => {
                    draft.resources_assets = normalize_unknown_list(parse_list_csv(trimmed_value));
                }
                "leader" => draft.leader = trimmed_value.to_string(),
                "allies" => draft.allies = normalize_unknown_list(parse_list_csv(trimmed_value)),
                "rivals_enemies" => {
                    draft.rivals_enemies = normalize_unknown_list(parse_list_csv(trimmed_value));
                }
                "liege" => draft.liege = Some(trimmed_value.to_string()),
                "loyalty_type" => {
                    draft.loyalty_type = Some(normalize_loyalty_type(trimmed_value)?);
                }
                _ => {}
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
                        public_description: draft.public_description.clone(),
                        reputation: draft.reputation.clone(),
                        symbol_description: draft.symbol_description.clone(),
                        want: draft.want.clone(),
                        obstacle: draft.obstacle.clone(),
                        action: draft.action.clone(),
                        consequence: draft.consequence.clone(),
                        leader: draft.leader.clone(),
                        sphere_of_influence: draft.sphere_of_influence.clone(),
                        resources_assets: draft.resources_assets.clone(),
                        allies: draft.allies.clone(),
                        rivals_enemies: draft.rivals_enemies.clone(),
                        liege: draft.liege.clone(),
                        loyalty_type: draft.loyalty_type.clone(),
                    },
                },
                database.as_ref(),
                generation_repo.as_ref(),
            )
            .await?;

        // Only the rerollable subset is generated (D3): the relational/place fields
        // (leader, allies, rivals, liege, loyalty_type) are never LLM-rerolled, so
        // they have no arms here.
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
                }
            }
            "public_description" => {
                if let Some(value) = rerolled.value {
                    draft.public_description = value;
                }
            }
            "reputation" => {
                if let Some(value) = rerolled.value {
                    draft.reputation = value;
                }
            }
            "symbol_description" => {
                if let Some(value) = rerolled.value {
                    draft.symbol_description = value;
                }
            }
            "want" => {
                if let Some(value) = rerolled.value {
                    draft.want = value;
                }
            }
            "obstacle" => {
                if let Some(value) = rerolled.value {
                    draft.obstacle = value;
                }
            }
            "action" => {
                if let Some(value) = rerolled.value {
                    draft.action = value;
                }
            }
            "consequence" => {
                if let Some(value) = rerolled.value {
                    draft.consequence = value;
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

pub fn faction_summary_text(draft: &FactionDraftSession) -> String {
    format!(
        "## Faction Draft\nname: {}\nslug: {}\nkind: {}\npublic: {}\nreputation: {}\nsymbol: {}\nwant: {}\nobstacle: {}\naction: {}\nconsequence: {}\nleader: {}\ninfluence: {}\nresources: {}\nallies: {}\nrivals: {}\nliege: {}\nloyalty: {}\npath: {}",
        draft.name,
        draft.slug,
        draft.kind_type,
        draft.public_description,
        draft.reputation,
        draft.symbol_description,
        draft.want,
        draft.obstacle,
        draft.action,
        draft.consequence,
        draft.leader,
        draft.sphere_of_influence,
        draft.resources_assets.join(", "),
        draft.allies.join(", "),
        draft.rivals_enemies.join(", "),
        draft.liege.as_deref().unwrap_or("(none)"),
        draft.loyalty_type.as_deref().unwrap_or("(none)"),
        draft.vault_path,
    )
}

pub fn faction_event_from_draft(draft: &FactionDraftSession) -> CommandClientEvent {
    use runebound_models::drafts::{CardFooter, faction_entity_card};

    let normalized_draft = FactionDraftSession {
        id: draft.id.clone(),
        seed_prompt: draft.seed_prompt.clone(),
        name: draft.name.clone(),
        slug: draft.slug.clone(),
        vault_path: draft.vault_path.clone(),
        kind_type: draft.kind_type.clone(),
        public_description: normalize_unknown_text(&draft.public_description),
        reputation: normalize_unknown_text(&draft.reputation),
        symbol_description: normalize_unknown_text(&draft.symbol_description),
        want: normalize_unknown_text(&draft.want),
        obstacle: normalize_unknown_text(&draft.obstacle),
        action: normalize_unknown_text(&draft.action),
        consequence: normalize_unknown_text(&draft.consequence),
        leader: normalize_unknown_text(&draft.leader),
        sphere_of_influence: normalize_unknown_text(&draft.sphere_of_influence),
        resources_assets: normalize_unknown_list(draft.resources_assets.clone()),
        allies: normalize_unknown_list(draft.allies.clone()),
        rivals_enemies: normalize_unknown_list(draft.rivals_enemies.clone()),
        // Relational stubs pass through as-is — the card renders Liege/Loyalty only
        // when set, so leave None as None rather than normalizing to "Unknown".
        liege: draft.liege.clone(),
        loyalty_type: draft.loyalty_type.clone(),
        wizard_subfoldered: draft.wizard_subfoldered,
    };
    let entity_card_doc = faction_entity_card(&normalized_draft, CardFooter::Show);
    CommandClientEvent::LoadFactionDraftWithCard {
        draft: normalized_draft,
        entity_card: entity_card_doc,
    }
}
