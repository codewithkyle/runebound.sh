use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use runebound_models::{FactionDraft, ItemDraft, LocationDraft, NpcDraft};

use crate::entities::{EntityDomainRegistry, EntityKind};
use crate::repositories::{
    Database, DocumentRepository, FactionRepository, GenerationRepository, ItemRepository,
    LocationRepository, NpcRepository, SoftDeleteRepository, VaultRepository,
};

pub type NpcDraftSession = NpcDraft;
pub type LocationDraftSession = LocationDraft;
pub type FactionDraftSession = FactionDraft;
pub type ItemDraftSession = ItemDraft;

#[derive(Debug, Clone)]
pub(crate) enum DraftEnvelope {
    Npc(NpcDraftSession),
    Location(LocationDraftSession),
    Faction(FactionDraftSession),
    Item(ItemDraftSession),
}

impl DraftEnvelope {
    pub(crate) fn kind(&self) -> EntityKind {
        match self {
            DraftEnvelope::Npc(_) => EntityKind::Npc,
            DraftEnvelope::Location(_) => EntityKind::Location,
            DraftEnvelope::Faction(_) => EntityKind::Faction,
            DraftEnvelope::Item(_) => EntityKind::Item,
        }
    }

    pub(crate) fn as_npc(&self) -> Option<&NpcDraftSession> {
        if let DraftEnvelope::Npc(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_npc_mut(&mut self) -> Option<&mut NpcDraftSession> {
        if let DraftEnvelope::Npc(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_location(&self) -> Option<&LocationDraftSession> {
        if let DraftEnvelope::Location(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_location_mut(&mut self) -> Option<&mut LocationDraftSession> {
        if let DraftEnvelope::Location(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_faction(&self) -> Option<&FactionDraftSession> {
        if let DraftEnvelope::Faction(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_faction_mut(&mut self) -> Option<&mut FactionDraftSession> {
        if let DraftEnvelope::Faction(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_item(&self) -> Option<&ItemDraftSession> {
        if let DraftEnvelope::Item(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_item_mut(&mut self) -> Option<&mut ItemDraftSession> {
        if let DraftEnvelope::Item(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

impl From<NpcDraftSession> for DraftEnvelope {
    fn from(draft: NpcDraftSession) -> Self {
        DraftEnvelope::Npc(draft)
    }
}

impl From<LocationDraftSession> for DraftEnvelope {
    fn from(draft: LocationDraftSession) -> Self {
        DraftEnvelope::Location(draft)
    }
}

impl From<FactionDraftSession> for DraftEnvelope {
    fn from(draft: FactionDraftSession) -> Self {
        DraftEnvelope::Faction(draft)
    }
}

impl From<ItemDraftSession> for DraftEnvelope {
    fn from(draft: ItemDraftSession) -> Self {
        DraftEnvelope::Item(draft)
    }
}

#[derive(Debug, Default)]
pub(crate) struct EditorSession {
    active_kind: Option<EntityKind>,
    drafts: HashMap<EntityKind, DraftEnvelope>,
}

impl EditorSession {
    pub(crate) fn active_kind(&self) -> Option<EntityKind> {
        self.active_kind
    }

    pub(crate) fn set_active_draft(&mut self, draft: DraftEnvelope) {
        let kind = draft.kind();
        self.drafts.insert(kind, draft);
        self.active_kind = Some(kind);
    }

    pub(crate) fn activate(&mut self, kind: EntityKind) {
        if self.drafts.contains_key(&kind) {
            self.active_kind = Some(kind);
        }
    }

    pub(crate) fn draft(&self, kind: EntityKind) -> Option<&DraftEnvelope> {
        self.drafts.get(&kind)
    }

    pub(crate) fn draft_mut(&mut self, kind: EntityKind) -> Option<&mut DraftEnvelope> {
        self.drafts.get_mut(&kind)
    }

    pub(crate) fn clear_kind(&mut self, kind: EntityKind) -> Option<DraftEnvelope> {
        let removed = self.drafts.remove(&kind);
        if self.active_kind == Some(kind) {
            self.active_kind = self.next_active_after(kind);
        }
        removed
    }

    pub(crate) fn clear_all(&mut self) {
        self.drafts.clear();
        self.active_kind = None;
    }

    pub(crate) fn get_npc(&self) -> Option<&NpcDraftSession> {
        self.draft(EntityKind::Npc).and_then(DraftEnvelope::as_npc)
    }

    pub(crate) fn get_npc_mut(&mut self) -> Option<&mut NpcDraftSession> {
        self.draft_mut(EntityKind::Npc)
            .and_then(DraftEnvelope::as_npc_mut)
    }

    pub(crate) fn set_npc(&mut self, draft: NpcDraftSession) {
        self.set_active_draft(DraftEnvelope::Npc(draft));
    }

    pub(crate) fn take_npc(&mut self) -> Option<NpcDraftSession> {
        self.clear_kind(EntityKind::Npc)
            .and_then(|envelope| match envelope {
                DraftEnvelope::Npc(draft) => Some(draft),
                _ => None,
            })
    }

    pub(crate) fn get_location(&self) -> Option<&LocationDraftSession> {
        self.draft(EntityKind::Location)
            .and_then(DraftEnvelope::as_location)
    }

    pub(crate) fn get_location_mut(&mut self) -> Option<&mut LocationDraftSession> {
        self.draft_mut(EntityKind::Location)
            .and_then(DraftEnvelope::as_location_mut)
    }

    pub(crate) fn set_location(&mut self, draft: LocationDraftSession) {
        self.set_active_draft(DraftEnvelope::Location(draft));
    }

    pub(crate) fn take_location(&mut self) -> Option<LocationDraftSession> {
        self.clear_kind(EntityKind::Location)
            .and_then(|envelope| match envelope {
                DraftEnvelope::Location(draft) => Some(draft),
                _ => None,
            })
    }

    pub(crate) fn get_faction(&self) -> Option<&FactionDraftSession> {
        self.draft(EntityKind::Faction)
            .and_then(DraftEnvelope::as_faction)
    }

    pub(crate) fn get_faction_mut(&mut self) -> Option<&mut FactionDraftSession> {
        self.draft_mut(EntityKind::Faction)
            .and_then(DraftEnvelope::as_faction_mut)
    }

    pub(crate) fn set_faction(&mut self, draft: FactionDraftSession) {
        self.set_active_draft(DraftEnvelope::Faction(draft));
    }

    pub(crate) fn take_faction(&mut self) -> Option<FactionDraftSession> {
        self.clear_kind(EntityKind::Faction)
            .and_then(|envelope| match envelope {
                DraftEnvelope::Faction(draft) => Some(draft),
                _ => None,
            })
    }

    pub(crate) fn get_item(&self) -> Option<&ItemDraftSession> {
        self.draft(EntityKind::Item).and_then(DraftEnvelope::as_item)
    }

    pub(crate) fn get_item_mut(&mut self) -> Option<&mut ItemDraftSession> {
        self.draft_mut(EntityKind::Item)
            .and_then(DraftEnvelope::as_item_mut)
    }

    pub(crate) fn set_item(&mut self, draft: ItemDraftSession) {
        self.set_active_draft(DraftEnvelope::Item(draft));
    }

    pub(crate) fn take_item(&mut self) -> Option<ItemDraftSession> {
        self.clear_kind(EntityKind::Item)
            .and_then(|envelope| match envelope {
                DraftEnvelope::Item(draft) => Some(draft),
                _ => None,
            })
    }

    fn next_active_after(&self, cleared: EntityKind) -> Option<EntityKind> {
        let search_order: &[EntityKind] = match cleared {
            EntityKind::Npc => &[EntityKind::Location, EntityKind::Faction, EntityKind::Item],
            EntityKind::Location => &[EntityKind::Npc, EntityKind::Faction, EntityKind::Item],
            EntityKind::Faction => &[EntityKind::Npc, EntityKind::Location, EntityKind::Item],
            EntityKind::Item => &[EntityKind::Npc, EntityKind::Location, EntityKind::Faction],
        };

        search_order
            .iter()
            .copied()
            .find(|kind| self.drafts.contains_key(kind))
    }
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
    pub(crate) item_repo: Arc<dyn ItemRepository>,
    pub(crate) document_repo: Arc<dyn DocumentRepository>,
    pub(crate) generation_repo: Arc<dyn GenerationRepository>,
    pub(crate) soft_delete_repo: Arc<dyn SoftDeleteRepository>,
    pub(crate) domains: Arc<EntityDomainRegistry>,
    /// Cached result of the boot LLM health probe, reused to render the MOTD
    /// without re-probing the Ollama server.
    pub(crate) boot_ollama_health: Mutex<Option<dnd_core::health::OllamaHealth>>,
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

    pub(crate) fn item_repo(&self) -> Arc<dyn ItemRepository> {
        self.item_repo.clone()
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

    pub(crate) fn domains(&self) -> Arc<EntityDomainRegistry> {
        self.domains.clone()
    }
}
