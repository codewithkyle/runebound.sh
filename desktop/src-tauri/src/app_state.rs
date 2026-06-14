use std::path::PathBuf;

use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub(crate) struct NpcDraftSession {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) race: String,
    pub(crate) occupation: String,
    pub(crate) sex: String,
    pub(crate) age: String,
    pub(crate) height: String,
    pub(crate) weight_lbs: String,
    pub(crate) background: String,
    pub(crate) want_need: String,
    pub(crate) secret_obstacle: String,
    pub(crate) carrying: Vec<String>,
    pub(crate) location: String,
}

#[derive(Debug, Clone)]
pub(crate) struct LocationDraftSession {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) slug: String,
    pub(crate) vault_path: String,
    pub(crate) kind_type: String,
    pub(crate) kind_custom: Option<String>,
    pub(crate) visual_description: String,
    pub(crate) history_background: String,
    pub(crate) exports: Vec<String>,
    pub(crate) tone: String,
    pub(crate) authority: String,
    pub(crate) danger_level: String,
    pub(crate) current_tension: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorMode {
    None,
    Npc,
    Location,
}

impl Default for EditorMode {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Default)]
pub(crate) struct EditorSession {
    pub(crate) mode: EditorMode,
    pub(crate) npc_draft: Option<NpcDraftSession>,
    pub(crate) location_draft: Option<LocationDraftSession>,
}

pub(crate) struct AppState {
    pub(crate) workspace_root: PathBuf,
    pub(crate) command_service: Mutex<dnd_core::service::CommandService>,
    pub(crate) editor_session: Mutex<EditorSession>,
}
