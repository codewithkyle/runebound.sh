use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::drafts::{
    DungeonDraft, EventDraft, FactionDraft, GodDraft, ItemDraft, LocationDraft, NpcDraft,
};
use super::output::OutputDoc;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandClientEvent {
    LoadNpcDraftWithCard {
        draft: NpcDraft,
        entity_card: OutputDoc,
    },
    LoadLocationDraftWithCard {
        draft: LocationDraft,
        entity_card: OutputDoc,
    },
    LoadFactionDraftWithCard {
        draft: FactionDraft,
        entity_card: OutputDoc,
    },
    LoadItemDraftWithCard {
        draft: ItemDraft,
        entity_card: OutputDoc,
    },
    LoadEventDraftWithCard {
        draft: EventDraft,
        entity_card: OutputDoc,
    },
    LoadGodDraftWithCard {
        draft: GodDraft,
        entity_card: OutputDoc,
    },
    LoadDungeonDraftWithCard {
        draft: DungeonDraft,
        entity_card: OutputDoc,
    },
    ClearDrafts,
    ClearTerminal {
        clear_history: bool,
    },
    ExitRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CommandResponse {
    pub ok: bool,
    pub output: String,
    pub error: Option<String>,
    pub exit_code: i32,
    pub segments: Vec<OutputSegment>,
    pub output_doc: Option<OutputDoc>,
    pub client_event: Option<CommandClientEvent>,
    /// Set when a multi-step wizard is active, so the frontend can drive the
    /// spinner from a structured signal instead of matching prompt text.
    pub wizard: Option<WizardView>,
}

/// Structured view of the active wizard step, returned alongside a wizard
/// prompt. Drives the frontend spinner label without prompt-text sniffing.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct WizardView {
    /// Active wizard id, e.g. "dungeon".
    pub id: String,
    /// Current step id, e.g. "plan_review".
    pub step_id: String,
    /// Spinner label to show when the user submits from this step (None = instant).
    pub awaiting_llm_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct OutputSegment {
    pub kind: OutputSegmentKind,
    pub text: String,
    pub command_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum OutputSegmentKind {
    Text,
    Error,
}
