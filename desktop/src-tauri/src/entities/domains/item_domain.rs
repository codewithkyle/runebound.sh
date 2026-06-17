use async_trait::async_trait;

use crate::app_state::{AppState, ItemDraftSession};
use crate::entities::EntityKind;
use crate::entities::common::{
    entity_message_response, entity_no_active_draft, entity_response_with_event,
    merge_seed_and_reroll_prompt, no_active_draft_message, normalize_unknown_list,
    normalize_unknown_text, parse_list_csv,
};
use crate::entities::domain::{EntityDomain, EntityDomainResult};
use crate::entities::schema::{
    FieldAccess, ITEM_SCHEMA, canonical_field_name, format_valid_field_list,
};
use crate::services::entity_persistence::{EntityPersistenceService, SaveItemDraftInput};
use crate::services::entity_reroll::{
    EntityRerollService, ItemRerollContext, RerollItemFieldInput,
};
use crate::utils::{normalize_item_category, normalize_item_rarity, path_for_display};
use dnd_core::command::CommandClientEvent;
use dnd_core::npc::slugify;

pub struct ItemDomain;

impl ItemDomain {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EntityDomain for ItemDomain {
    fn kind(&self) -> EntityKind {
        EntityKind::Item
    }

    fn schema(&self) -> &'static crate::entities::schema::EntitySchema {
        &ITEM_SCHEMA
    }

    fn help_text(&self) -> String {
        [
            "## Item editor commands",
            "item show",
            "item rename <name>",
            "item set <field> <value>",
            "item reroll <field> [prompt]",
            "reroll",
            "item save",
            "item cancel",
        ]
        .join("\n")
    }

    async fn show_draft(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_item().cloned()
        };

        let Some(draft) = draft else {
            return entity_no_active_draft(EntityKind::Item);
        };

        entity_response_with_event(item_summary_text(&draft), item_event_from_draft(&draft))
    }

    async fn rename(&self, value: &str, state: &AppState) -> EntityDomainResult {
        let name = value.trim();
        if name.is_empty() {
            return entity_message_response("item name cannot be empty.");
        }

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor
                .get_item_mut()
                .ok_or_else(|| no_active_draft_message(EntityKind::Item))?;
            draft.name = name.to_string();
            draft.slug = slugify(name);
            let snapshot = draft.clone();
            editor.activate(EntityKind::Item);
            snapshot
        };

        entity_response_with_event(item_summary_text(&updated), item_event_from_draft(&updated))
    }

    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> EntityDomainResult {
        let trimmed_value = value.trim();
        if trimmed_value.is_empty() {
            return entity_message_response("item set value cannot be empty.");
        }

        let Some(canonical) = canonical_field_name(EntityKind::Item, field, FieldAccess::Set)
        else {
            let valid_fields = format_valid_field_list(EntityKind::Item, FieldAccess::Set);
            return entity_message_response(format!(
                "unknown item field: {}. valid fields: {}",
                field, valid_fields
            ));
        };

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor
                .get_item_mut()
                .ok_or_else(|| no_active_draft_message(EntityKind::Item))?;

            match canonical {
                "name" => {
                    draft.name = trimmed_value.to_string();
                    draft.slug = slugify(trimmed_value);
                }
                "category" => draft.category = normalize_item_category(trimmed_value)?,
                "rarity" => draft.rarity = normalize_item_rarity(trimmed_value)?,
                "attunement" => draft.attunement = normalize_unknown_text(trimmed_value),
                "materials" => {
                    draft.materials = normalize_unknown_list(parse_list_csv(trimmed_value))
                }
                "appearance" => draft.appearance = normalize_unknown_text(trimmed_value),
                "abilities" => draft.abilities = normalize_unknown_text(trimmed_value),
                "drawbacks" => draft.drawbacks = normalize_unknown_text(trimmed_value),
                "history" => draft.history = normalize_unknown_text(trimmed_value),
                "value" => draft.value = normalize_unknown_text(trimmed_value),
                "location" => draft.location = normalize_unknown_text(trimmed_value),
                _ => {}
            }

            let snapshot = draft.clone();
            editor.activate(EntityKind::Item);
            snapshot
        };

        entity_response_with_event(item_summary_text(&updated), item_event_from_draft(&updated))
    }

    async fn reroll_field(
        &self,
        field: &str,
        prompt: Option<String>,
        state: &AppState,
    ) -> EntityDomainResult {
        if field.trim().is_empty() {
            return entity_message_response("usage: item reroll <field> [prompt]");
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.get_item().cloned()
        }
        .ok_or_else(|| no_active_draft_message(EntityKind::Item))?;

        let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

        let reroll_service = EntityRerollService;
        let workspace_root = state.workspace_root.clone();
        let rerolled = reroll_service
            .reroll_item_field(
                RerollItemFieldInput {
                    field: field.to_string(),
                    prompt,
                    item: ItemRerollContext {
                        name: draft.name.clone(),
                        category: draft.category.clone(),
                        rarity: draft.rarity.clone(),
                        attunement: draft.attunement.clone(),
                        materials: draft.materials.clone(),
                        appearance: draft.appearance.clone(),
                        abilities: draft.abilities.clone(),
                        drawbacks: draft.drawbacks.clone(),
                        history: draft.history.clone(),
                        value: draft.value.clone(),
                        location: draft.location.clone(),
                    },
                },
                &workspace_root,
                state.database().as_ref(),
                state.generation_repo().as_ref(),
            )
            .await?;

        match rerolled.field.as_str() {
            "name" => {
                if let Some(value) = rerolled.value {
                    draft.slug = slugify(&value);
                    draft.name = value;
                }
            }
            "category" => {
                if let Some(value) = rerolled.value {
                    draft.category = value;
                }
            }
            "rarity" => {
                if let Some(value) = rerolled.value {
                    draft.rarity = value;
                }
            }
            "attunement" => {
                if let Some(value) = rerolled.value {
                    draft.attunement = value;
                }
            }
            "materials" => {
                if let Some(materials) = rerolled.materials {
                    draft.materials = materials;
                }
            }
            "appearance" => {
                if let Some(value) = rerolled.value {
                    draft.appearance = value;
                }
            }
            "abilities" => {
                if let Some(value) = rerolled.value {
                    draft.abilities = value;
                }
            }
            "drawbacks" => {
                if let Some(value) = rerolled.value {
                    draft.drawbacks = value;
                }
            }
            "history" => {
                if let Some(value) = rerolled.value {
                    draft.history = value;
                }
            }
            "value" => {
                if let Some(value) = rerolled.value {
                    draft.value = value;
                }
            }
            "location" => {
                if let Some(value) = rerolled.value {
                    draft.location = value;
                }
            }
            _ => {}
        }

        {
            let mut editor = state.editor_session.lock().await;
            editor.set_item(draft.clone());
        }

        entity_response_with_event(item_summary_text(&draft), item_event_from_draft(&draft))
    }

    async fn save(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_item().cloned()
        }
        .ok_or_else(|| no_active_draft_message(EntityKind::Item))?;

        let persistence = EntityPersistenceService;
        let result = persistence
            .save_item_draft(
                SaveItemDraftInput {
                    id: draft.id.clone(),
                    name: draft.name.clone(),
                    category: draft.category.clone(),
                    rarity: draft.rarity.clone(),
                    attunement: draft.attunement.clone(),
                    materials: draft.materials.clone(),
                    appearance: draft.appearance.clone(),
                    abilities: draft.abilities.clone(),
                    drawbacks: draft.drawbacks.clone(),
                    history: draft.history.clone(),
                    value: draft.value.clone(),
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
            "## Item saved".to_string(),
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
            editor.take_item()
        };

        if removed.is_none() {
            return entity_no_active_draft(EntityKind::Item);
        }

        entity_response_with_event("item draft discarded.", CommandClientEvent::ClearDrafts)
    }
}

pub fn item_summary_text(draft: &ItemDraftSession) -> String {
    format!(
        "## Item Draft\nname: {}\nslug: {}\ncategory: {}\nrarity: {}\nattunement: {}\nmaterials: {}\nappearance: {}\nabilities: {}\ndrawbacks: {}\nhistory: {}\nvalue: {}\nlocation: {}",
        draft.name,
        draft.slug,
        draft.category,
        draft.rarity,
        draft.attunement,
        draft.materials.join(", "),
        draft.appearance,
        draft.abilities,
        draft.drawbacks,
        draft.history,
        draft.value,
        draft.location,
    )
}

pub fn item_event_from_draft(draft: &ItemDraftSession) -> CommandClientEvent {
    use runebound_models::drafts::item_entity_card;

    let normalized = ItemDraftSession {
        id: draft.id.clone(),
        seed_prompt: draft.seed_prompt.clone(),
        name: draft.name.clone(),
        slug: draft.slug.clone(),
        vault_path: draft.vault_path.clone(),
        category: draft.category.clone(),
        rarity: draft.rarity.clone(),
        attunement: normalize_unknown_text(&draft.attunement),
        materials: normalize_unknown_list(draft.materials.clone()),
        appearance: normalize_unknown_text(&draft.appearance),
        abilities: normalize_unknown_text(&draft.abilities),
        drawbacks: normalize_unknown_text(&draft.drawbacks),
        history: normalize_unknown_text(&draft.history),
        value: normalize_unknown_text(&draft.value),
        location: normalize_unknown_text(&draft.location),
    };
    let entity_card = item_entity_card(&normalized);
    CommandClientEvent::LoadItemDraftWithCard {
        draft: normalized,
        entity_card,
    }
}
