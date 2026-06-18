use async_trait::async_trait;

use crate::app_state::{AppState, DraftEnvelope, EventDraftSession};
use crate::entities::EntityKind;
use crate::entities::common::{
    entity_message_response, entity_no_active_draft, entity_response_with_event,
    merge_seed_and_reroll_prompt, no_active_draft_message,
};
use crate::entities::domain::{EntityDetail, EntityDomain, EntityDomainResult};
use crate::entities::schema::EVENT_SCHEMA;
use crate::services::ai_generation::{AiGenerationService, SeedGeneration};
use crate::utils::prepend_notice;
use dnd_core::command::CommandClientEvent;
use dnd_core::npc::slugify;

pub struct EventDomain;

impl EventDomain {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EntityDomain for EventDomain {
    fn kind(&self) -> EntityKind {
        EntityKind::Event
    }

    fn schema(&self) -> &'static crate::entities::schema::EntitySchema {
        &EVENT_SCHEMA
    }

    fn help_text(&self) -> String {
        [
            "## Event editor commands",
            "event show",
            "event reroll [prompt]",
            "reroll",
            "event save",
            "event cancel",
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
            .event_repo()
            .find_by_name_or_slug(database.as_ref(), name_or_slug)
            .await?
        else {
            return Ok(None);
        };
        let draft = EventDraftSession {
            id: row.id,
            seed_prompt: None,
            name: row.name,
            slug: row.slug,
            body: row.body,
        };
        Ok(Some(EntityDetail {
            draft: DraftEnvelope::Event(draft),
        }))
    }

    async fn show_draft(&self, state: &AppState) -> EntityDomainResult {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_event().cloned()
        };

        let Some(draft) = draft else {
            return entity_no_active_draft(EntityKind::Event);
        };

        entity_response_with_event(event_summary_text(&draft), event_event_from_draft(&draft))
    }

    async fn rename(&self, value: &str, state: &AppState) -> EntityDomainResult {
        let name = value.trim();
        if name.is_empty() {
            return entity_message_response("event name cannot be empty.");
        }

        let updated = {
            let mut editor = state.editor_session.lock().await;
            let draft = editor
                .get_event_mut()
                .ok_or_else(|| no_active_draft_message(EntityKind::Event))?;
            draft.name = name.to_string();
            draft.slug = slugify(name);
            draft.clone()
        };

        entity_response_with_event(
            event_summary_text(&updated),
            event_event_from_draft(&updated),
        )
    }

    async fn set_field(&self, _field: &str, _value: &str, _state: &AppState) -> EntityDomainResult {
        // Events are narrative-only: there are no attributes to set. Regenerating
        // the whole story is the only way to change its contents.
        entity_message_response(
            "events have no fields to set. use `event reroll [prompt]` to regenerate the narrative.",
        )
    }

    async fn reroll_field(
        &self,
        _field: &str,
        prompt: Option<String>,
        state: &AppState,
    ) -> EntityDomainResult {
        // Events have no per-field reroll: any `reroll` regenerates the entire
        // narrative, so the field argument is intentionally ignored.
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.get_event().cloned()
        }
        .ok_or_else(|| no_active_draft_message(EntityKind::Event))?;

        let merged_prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

        let ai = AiGenerationService;
        let SeedGeneration { seed, notice } = ai
            .generate_event_seed(
                merged_prompt,
                state.database().as_ref(),
                state.generation_repo().as_ref(),
            )
            .await?;

        let updated = EventDraftSession {
            id: draft.id.clone(),
            seed_prompt: draft.seed_prompt.clone(),
            name: seed.title.trim().to_string(),
            slug: slugify(seed.title.trim()),
            body: seed.body.trim().to_string(),
        };

        {
            let mut editor = state.editor_session.lock().await;
            editor.set_event(updated.clone());
        }

        entity_response_with_event(
            prepend_notice(notice, event_summary_text(&updated)),
            event_event_from_draft(&updated),
        )
    }

    async fn cancel(&self, state: &AppState) -> EntityDomainResult {
        let removed = {
            let mut editor = state.editor_session.lock().await;
            editor.take_event()
        };

        if removed.is_none() {
            return entity_no_active_draft(EntityKind::Event);
        }

        entity_response_with_event("event draft discarded.", CommandClientEvent::ClearDrafts)
    }
}

pub fn event_summary_text(draft: &EventDraftSession) -> String {
    format!(
        "## Event Draft\nname: {}\nslug: {}\n\n{}",
        draft.name, draft.slug, draft.body,
    )
}

pub fn event_event_from_draft(draft: &EventDraftSession) -> CommandClientEvent {
    use runebound_models::drafts::event_entity_card;

    let entity_card = event_entity_card(draft);
    CommandClientEvent::LoadEventDraftWithCard {
        draft: draft.clone(),
        entity_card,
    }
}
