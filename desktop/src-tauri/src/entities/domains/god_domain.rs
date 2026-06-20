use async_trait::async_trait;

use crate::app_state::{AppState, DraftEnvelope, GodDraftSession};
use crate::entities::EntityKind;
use crate::entities::common::{
    entity_message_response, entity_no_active_draft, entity_response_with_event,
    merge_seed_and_reroll_prompt, normalize_unknown_list, normalize_unknown_text, parse_list_csv,
};
use crate::entities::domain::{EntityDetail, EntityDomain, EntityDomainResult};
use crate::entities::schema::{FieldAccess, canonical_field_name, format_valid_field_list};
use crate::services::entity_reroll::{EntityRerollService, GodRerollContext, RerollGodFieldInput};
use crate::utils::{
    normalize_god_alignment, normalize_god_rank, normalize_optional_prompt, path_for_display,
};
use dnd_core::command::CommandClientEvent;
use dnd_core::npc::slugify;
use dnd_core::serialization::faction_list_from_db_text;

pub struct GodDomain;

impl GodDomain {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EntityDomain for GodDomain {
    fn kind(&self) -> EntityKind {
        EntityKind::God
    }

    fn help_text(&self) -> String {
        [
            "## God editor commands",
            "god show",
            "god rename <name>",
            "god set <field> <value>",
            "god reroll <field> [prompt]",
            "reroll",
            "god save",
            "god cancel",
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
            .god_repo()
            .find_by_name_or_slug(database.as_ref(), name_or_slug)
            .await?
        else {
            return Ok(None);
        };
        let draft = GodDraftSession {
            id: row.id,
            seed_prompt: None,
            name: row.name,
            slug: row.slug,
            vault_path: path_for_display(&row.vault_path),
            epithet: row.epithet,
            rank: row.rank,
            rank_custom: row.rank_custom,
            alignment: row.alignment,
            domains: faction_list_from_db_text(&row.domains),
            symbol: row.symbol,
            appearance: row.appearance,
            dogma: row.dogma,
            realm: row.realm,
            worshippers: row.worshippers,
            clergy: row.clergy,
            allies: faction_list_from_db_text(&row.allies),
            rivals: faction_list_from_db_text(&row.rivals),
        };
        Ok(Some(EntityDetail {
            draft: DraftEnvelope::God(draft),
        }))
    }

    async fn show_draft(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_god().cloned()
        };
        let Some(draft) = draft else {
            return entity_no_active_draft(EntityKind::God);
        };

        entity_response_with_event(god_summary_text(&draft), god_event_from_draft(&draft))
    }

    async fn rename(&self, value: &str, state: &AppState) -> EntityDomainResult {
        let name = value.trim();
        if name.is_empty() {
            return entity_message_response("god name cannot be empty.");
        }

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor
                .get_god_mut()
                .ok_or_else(|| "no active god draft. run create god or load <name>.".to_string())?;
            draft.name = name.to_string();
            draft.slug = slugify(name);
            draft.clone()
        };

        entity_response_with_event(god_summary_text(&updated), god_event_from_draft(&updated))
    }

    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> EntityDomainResult {
        let trimmed_value = value.trim();
        if trimmed_value.is_empty() {
            return entity_message_response("god set value cannot be empty.");
        }

        let canonical =
            canonical_field_name(EntityKind::God, field, FieldAccess::Set).ok_or_else(|| {
                let valid_fields = format_valid_field_list(EntityKind::God, FieldAccess::Set);
                format!(
                    "unknown god set field: {}. valid fields: {}",
                    field, valid_fields
                )
            })?;

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor
                .get_god_mut()
                .ok_or_else(|| "no active god draft. run create god or load <name>.".to_string())?;

            match canonical {
                "name" => {
                    draft.name = trimmed_value.to_string();
                    draft.slug = slugify(trimmed_value);
                }
                "epithet" => draft.epithet = trimmed_value.to_string(),
                "rank" => {
                    draft.rank = normalize_god_rank(trimmed_value)?;
                    if draft.rank == "other" && draft.rank_custom.is_none() {
                        draft.rank_custom = Some("Unknown".to_string());
                    }
                }
                "rank_custom" => draft.rank_custom = Some(trimmed_value.to_string()),
                "alignment" => draft.alignment = normalize_god_alignment(trimmed_value)?,
                "domains" => draft.domains = normalize_unknown_list(parse_list_csv(trimmed_value)),
                "symbol" => draft.symbol = trimmed_value.to_string(),
                "appearance" => draft.appearance = trimmed_value.to_string(),
                "dogma" => draft.dogma = trimmed_value.to_string(),
                "realm" => draft.realm = trimmed_value.to_string(),
                "worshippers" => draft.worshippers = trimmed_value.to_string(),
                "clergy" => draft.clergy = trimmed_value.to_string(),
                "allies" => draft.allies = normalize_unknown_list(parse_list_csv(trimmed_value)),
                "rivals" => draft.rivals = normalize_unknown_list(parse_list_csv(trimmed_value)),
                _ => {}
            }

            if draft.rank == "other"
                && draft
                    .rank_custom
                    .as_ref()
                    .is_none_or(|item| item.trim().is_empty())
            {
                return entity_message_response(
                    "rank_custom is required when rank is other. use god set rank_custom <value>.",
                );
            }
            if draft.rank != "other" {
                draft.rank_custom = None;
            }

            draft.clone()
        };

        entity_response_with_event(god_summary_text(&updated), god_event_from_draft(&updated))
    }

    async fn reroll_field(
        &self,
        field: &str,
        prompt: Option<String>,
        state: &AppState,
    ) -> EntityDomainResult {
        if field.trim().is_empty() {
            return entity_message_response("usage: god reroll <field> [prompt]");
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.get_god().cloned()
        }
        .ok_or_else(|| "no active god draft. run create god or load <name>.".to_string())?;

        let prompt = normalize_optional_prompt(prompt).map(|value| value.to_string());

        let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

        let reroll_service = EntityRerollService;
        let database = state.database();
        let generation_repo = state.generation_repo();
        let rerolled = reroll_service
            .reroll_god_field(
                RerollGodFieldInput {
                    field: field.to_string(),
                    prompt,
                    god: GodRerollContext {
                        name: draft.name.clone(),
                        epithet: draft.epithet.clone(),
                        rank: draft.rank.clone(),
                        rank_custom: draft.rank_custom.clone(),
                        alignment: draft.alignment.clone(),
                        domains: draft.domains.clone(),
                        symbol: draft.symbol.clone(),
                        appearance: draft.appearance.clone(),
                        dogma: draft.dogma.clone(),
                        realm: draft.realm.clone(),
                        worshippers: draft.worshippers.clone(),
                        clergy: draft.clergy.clone(),
                        allies: draft.allies.clone(),
                        rivals: draft.rivals.clone(),
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
            "epithet" => {
                if let Some(value) = rerolled.value {
                    draft.epithet = value;
                }
            }
            "rank" => {
                if let Some(value) = rerolled.value {
                    draft.rank = normalize_god_rank(&value)?;
                    if draft.rank != "other" {
                        draft.rank_custom = None;
                    } else if draft.rank_custom.is_none() {
                        draft.rank_custom = Some("Unknown".to_string());
                    }
                }
            }
            "rank_custom" => {
                if let Some(value) = rerolled.value {
                    draft.rank_custom = Some(value);
                }
            }
            "alignment" => {
                if let Some(value) = rerolled.value {
                    draft.alignment = normalize_god_alignment(&value)?;
                }
            }
            "domains" => {
                if let Some(value) = rerolled.list_value {
                    draft.domains = value;
                }
            }
            "symbol" => {
                if let Some(value) = rerolled.value {
                    draft.symbol = value;
                }
            }
            "appearance" => {
                if let Some(value) = rerolled.value {
                    draft.appearance = value;
                }
            }
            "dogma" => {
                if let Some(value) = rerolled.value {
                    draft.dogma = value;
                }
            }
            "realm" => {
                if let Some(value) = rerolled.value {
                    draft.realm = value;
                }
            }
            "worshippers" => {
                if let Some(value) = rerolled.value {
                    draft.worshippers = value;
                }
            }
            "clergy" => {
                if let Some(value) = rerolled.value {
                    draft.clergy = value;
                }
            }
            "allies" => {
                if let Some(value) = rerolled.list_value {
                    draft.allies = value;
                }
            }
            "rivals" => {
                if let Some(value) = rerolled.list_value {
                    draft.rivals = value;
                }
            }
            _ => {}
        }

        {
            let mut editor = state.editor_session.lock().await;
            editor.set_god(draft.clone());
        }

        entity_response_with_event(god_summary_text(&draft), god_event_from_draft(&draft))
    }

    async fn cancel(&self, state: &AppState) -> EntityDomainResult {
        let removed = {
            let mut editor = state.editor_session.lock().await;
            editor.take_god()
        };
        if removed.is_none() {
            return entity_no_active_draft(EntityKind::God);
        }

        entity_response_with_event("god draft discarded.", CommandClientEvent::ClearDrafts)
    }
}

pub fn god_summary_text(draft: &GodDraftSession) -> String {
    format!(
        "## God Draft\nname: {}\nslug: {}\nepithet: {}\nrank: {}\nrank_custom: {}\nalignment: {}\ndomains: {}\nsymbol: {}\nappearance: {}\ndogma: {}\nrealm: {}\nworshippers: {}\nclergy: {}\nallies: {}\nrivals: {}\npath: {}",
        draft.name,
        draft.slug,
        draft.epithet,
        draft.rank,
        draft.rank_custom.as_deref().unwrap_or("(none)"),
        draft.alignment,
        draft.domains.join(", "),
        draft.symbol,
        draft.appearance,
        draft.dogma,
        draft.realm,
        draft.worshippers,
        draft.clergy,
        draft.allies.join(", "),
        draft.rivals.join(", "),
        draft.vault_path,
    )
}

pub fn god_event_from_draft(draft: &GodDraftSession) -> CommandClientEvent {
    use runebound_models::drafts::{CardFooter, god_entity_card};

    let normalized_draft = GodDraftSession {
        id: draft.id.clone(),
        name: draft.name.clone(),
        slug: draft.slug.clone(),
        vault_path: draft.vault_path.clone(),
        epithet: normalize_unknown_text(&draft.epithet),
        rank: draft.rank.clone(),
        rank_custom: draft.rank_custom.clone(),
        alignment: draft.alignment.clone(),
        domains: normalize_unknown_list(draft.domains.clone()),
        symbol: normalize_unknown_text(&draft.symbol),
        appearance: normalize_unknown_text(&draft.appearance),
        dogma: normalize_unknown_text(&draft.dogma),
        realm: normalize_unknown_text(&draft.realm),
        worshippers: normalize_unknown_text(&draft.worshippers),
        clergy: normalize_unknown_text(&draft.clergy),
        allies: normalize_unknown_list(draft.allies.clone()),
        rivals: normalize_unknown_list(draft.rivals.clone()),
        seed_prompt: draft.seed_prompt.clone(),
    };
    let entity_card_doc = god_entity_card(&normalized_draft, CardFooter::Show);
    CommandClientEvent::LoadGodDraftWithCard {
        draft: normalized_draft,
        entity_card: entity_card_doc,
    }
}
