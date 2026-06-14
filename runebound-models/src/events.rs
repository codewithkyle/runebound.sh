use serde::{Deserialize, Serialize};

use super::drafts::{FactionDraft, LocationDraft, NpcDraft};
use super::output::OutputDoc;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    ClearDrafts,
    ClearTerminal {
        clear_history: bool,
    },
    ExitRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResponse {
    pub ok: bool,
    pub output: String,
    pub error: Option<String>,
    pub exit_code: i32,
    pub segments: Vec<OutputSegment>,
    pub output_doc: Option<OutputDoc>,
    pub client_event: Option<CommandClientEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSegment {
    pub kind: OutputSegmentKind,
    pub text: String,
    pub command_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputSegmentKind {
    Text,
    Error,
}
