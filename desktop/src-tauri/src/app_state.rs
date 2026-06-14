use std::path::PathBuf;

use tokio::sync::Mutex;

use runebound_models::{FactionDraft, LocationDraft, NpcDraft};

pub type NpcDraftSession = NpcDraft;
pub type LocationDraftSession = LocationDraft;
pub type FactionDraftSession = FactionDraft;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorMode {
    None,
    Npc,
    Location,
    Faction,
}

impl Default for EditorMode {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Default)]
pub(crate) struct EditorSession {
    pub(crate) mode: EditorMode,
    pub(crate) npc_draft: Option<NpcDraft>,
    pub(crate) location_draft: Option<LocationDraft>,
    pub(crate) faction_draft: Option<FactionDraft>,
}

pub(crate) struct AppState {
    pub(crate) workspace_root: PathBuf,
    pub(crate) command_service: Mutex<dnd_core::service::CommandService>,
    pub(crate) editor_session: Mutex<EditorSession>,
}