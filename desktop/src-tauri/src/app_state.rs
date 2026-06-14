use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use runebound_models::{FactionDraft, LocationDraft, NpcDraft};
use crate::repositories::{
    Database, DocumentRepository, FactionRepository, GenerationRepository, LocationRepository,
    NpcRepository, SoftDeleteRepository, VaultRepository,
};

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
    pub(crate) database: Arc<Database>,
    pub(crate) vault_repo: Arc<dyn VaultRepository>,
    pub(crate) npc_repo: Arc<dyn NpcRepository>,
    pub(crate) location_repo: Arc<dyn LocationRepository>,
    pub(crate) faction_repo: Arc<dyn FactionRepository>,
    pub(crate) document_repo: Arc<dyn DocumentRepository>,
    pub(crate) generation_repo: Arc<dyn GenerationRepository>,
    pub(crate) soft_delete_repo: Arc<dyn SoftDeleteRepository>,
}

impl AppState {
    pub(crate) fn database(&self) -> Arc<Database> {
        self.database.clone()
    }

    pub(crate) fn vault_repo(&self) -> Arc<dyn VaultRepository> {
        self.vault_repo.clone()
    }

    pub(crate) fn npc_repo(&self) -> Arc<dyn NpcRepository> {
        self.npc_repo.clone()
    }

    pub(crate) fn location_repo(&self) -> Arc<dyn LocationRepository> {
        self.location_repo.clone()
    }

    pub(crate) fn faction_repo(&self) -> Arc<dyn FactionRepository> {
        self.faction_repo.clone()
    }

    pub(crate) fn document_repo(&self) -> Arc<dyn DocumentRepository> {
        self.document_repo.clone()
    }

    pub(crate) fn generation_repo(&self) -> Arc<dyn GenerationRepository> {
        self.generation_repo.clone()
    }

    pub(crate) fn soft_delete_repo(&self) -> Arc<dyn SoftDeleteRepository> {
        self.soft_delete_repo.clone()
    }
}
