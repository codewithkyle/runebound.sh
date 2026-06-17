use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use dnd_core::command_manifest::InputContext;
use runebound_models::{
    DungeonDraft, EventDraft, FactionDraft, GodDraft, ItemDraft, LocationDraft, NpcDraft,
};

use crate::entities::{EntityDomainRegistry, EntityKind};
use crate::wizards::{WizardRegistry, WizardSession};
use crate::repositories::{
    Database, DocumentRepository, DungeonRepository, EventRepository, FactionRepository,
    GenerationRepository, GodRepository, ItemRepository, LocationRepository, NpcRepository,
    SoftDeleteRepository, VaultRepository,
};

pub type NpcDraftSession = NpcDraft;
pub type LocationDraftSession = LocationDraft;
pub type FactionDraftSession = FactionDraft;
pub type ItemDraftSession = ItemDraft;
pub type EventDraftSession = EventDraft;
pub type GodDraftSession = GodDraft;
pub type DungeonDraftSession = DungeonDraft;

#[derive(Debug, Clone)]
pub(crate) enum DraftEnvelope {
    Npc(NpcDraftSession),
    Location(LocationDraftSession),
    Faction(FactionDraftSession),
    Item(ItemDraftSession),
    Event(EventDraftSession),
    God(GodDraftSession),
    Dungeon(DungeonDraftSession),
}

impl DraftEnvelope {
    pub(crate) fn kind(&self) -> EntityKind {
        match self {
            DraftEnvelope::Npc(_) => EntityKind::Npc,
            DraftEnvelope::Location(_) => EntityKind::Location,
            DraftEnvelope::Faction(_) => EntityKind::Faction,
            DraftEnvelope::Item(_) => EntityKind::Item,
            DraftEnvelope::Event(_) => EntityKind::Event,
            DraftEnvelope::God(_) => EntityKind::God,
            DraftEnvelope::Dungeon(_) => EntityKind::Dungeon,
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

    pub(crate) fn as_event(&self) -> Option<&EventDraftSession> {
        if let DraftEnvelope::Event(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_event_mut(&mut self) -> Option<&mut EventDraftSession> {
        if let DraftEnvelope::Event(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_god(&self) -> Option<&GodDraftSession> {
        if let DraftEnvelope::God(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_god_mut(&mut self) -> Option<&mut GodDraftSession> {
        if let DraftEnvelope::God(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_dungeon(&self) -> Option<&DungeonDraftSession> {
        if let DraftEnvelope::Dungeon(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(crate) fn as_dungeon_mut(&mut self) -> Option<&mut DungeonDraftSession> {
        if let DraftEnvelope::Dungeon(value) = self {
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

impl From<EventDraftSession> for DraftEnvelope {
    fn from(draft: EventDraftSession) -> Self {
        DraftEnvelope::Event(draft)
    }
}

impl From<GodDraftSession> for DraftEnvelope {
    fn from(draft: GodDraftSession) -> Self {
        DraftEnvelope::God(draft)
    }
}

impl From<DungeonDraftSession> for DraftEnvelope {
    fn from(draft: DungeonDraftSession) -> Self {
        DraftEnvelope::Dungeon(draft)
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

    pub(crate) fn get_event(&self) -> Option<&EventDraftSession> {
        self.draft(EntityKind::Event)
            .and_then(DraftEnvelope::as_event)
    }

    #[allow(dead_code)]
    pub(crate) fn get_event_mut(&mut self) -> Option<&mut EventDraftSession> {
        self.draft_mut(EntityKind::Event)
            .and_then(DraftEnvelope::as_event_mut)
    }

    pub(crate) fn set_event(&mut self, draft: EventDraftSession) {
        self.set_active_draft(DraftEnvelope::Event(draft));
    }

    pub(crate) fn take_event(&mut self) -> Option<EventDraftSession> {
        self.clear_kind(EntityKind::Event)
            .and_then(|envelope| match envelope {
                DraftEnvelope::Event(draft) => Some(draft),
                _ => None,
            })
    }

    pub(crate) fn get_god(&self) -> Option<&GodDraftSession> {
        self.draft(EntityKind::God).and_then(DraftEnvelope::as_god)
    }

    pub(crate) fn get_god_mut(&mut self) -> Option<&mut GodDraftSession> {
        self.draft_mut(EntityKind::God)
            .and_then(DraftEnvelope::as_god_mut)
    }

    pub(crate) fn set_god(&mut self, draft: GodDraftSession) {
        self.set_active_draft(DraftEnvelope::God(draft));
    }

    pub(crate) fn take_god(&mut self) -> Option<GodDraftSession> {
        self.clear_kind(EntityKind::God)
            .and_then(|envelope| match envelope {
                DraftEnvelope::God(draft) => Some(draft),
                _ => None,
            })
    }

    pub(crate) fn get_dungeon(&self) -> Option<&DungeonDraftSession> {
        self.draft(EntityKind::Dungeon)
            .and_then(DraftEnvelope::as_dungeon)
    }

    pub(crate) fn get_dungeon_mut(&mut self) -> Option<&mut DungeonDraftSession> {
        self.draft_mut(EntityKind::Dungeon)
            .and_then(DraftEnvelope::as_dungeon_mut)
    }

    pub(crate) fn set_dungeon(&mut self, draft: DungeonDraftSession) {
        self.set_active_draft(DraftEnvelope::Dungeon(draft));
    }

    pub(crate) fn take_dungeon(&mut self) -> Option<DungeonDraftSession> {
        self.clear_kind(EntityKind::Dungeon)
            .and_then(|envelope| match envelope {
                DraftEnvelope::Dungeon(draft) => Some(draft),
                _ => None,
            })
    }

    fn next_active_after(&self, cleared: EntityKind) -> Option<EntityKind> {
        let search_order: &[EntityKind] = match cleared {
            EntityKind::Npc => &[
                EntityKind::Location,
                EntityKind::Faction,
                EntityKind::Item,
                EntityKind::Event,
                EntityKind::God,
                EntityKind::Dungeon,
            ],
            EntityKind::Location => &[
                EntityKind::Npc,
                EntityKind::Faction,
                EntityKind::Item,
                EntityKind::Event,
                EntityKind::God,
                EntityKind::Dungeon,
            ],
            EntityKind::Faction => &[
                EntityKind::Npc,
                EntityKind::Location,
                EntityKind::Item,
                EntityKind::Event,
                EntityKind::God,
                EntityKind::Dungeon,
            ],
            EntityKind::Item => &[
                EntityKind::Npc,
                EntityKind::Location,
                EntityKind::Faction,
                EntityKind::Event,
                EntityKind::God,
                EntityKind::Dungeon,
            ],
            EntityKind::Event => &[
                EntityKind::Npc,
                EntityKind::Location,
                EntityKind::Faction,
                EntityKind::Item,
                EntityKind::God,
                EntityKind::Dungeon,
            ],
            EntityKind::God => &[
                EntityKind::Npc,
                EntityKind::Location,
                EntityKind::Faction,
                EntityKind::Item,
                EntityKind::Event,
                EntityKind::Dungeon,
            ],
            EntityKind::Dungeon => &[
                EntityKind::Npc,
                EntityKind::Location,
                EntityKind::Faction,
                EntityKind::Item,
                EntityKind::Event,
                EntityKind::God,
            ],
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
    pub(crate) event_repo: Arc<dyn EventRepository>,
    pub(crate) god_repo: Arc<dyn GodRepository>,
    pub(crate) dungeon_repo: Arc<dyn DungeonRepository>,
    pub(crate) document_repo: Arc<dyn DocumentRepository>,
    pub(crate) generation_repo: Arc<dyn GenerationRepository>,
    pub(crate) soft_delete_repo: Arc<dyn SoftDeleteRepository>,
    pub(crate) domains: Arc<EntityDomainRegistry>,
    /// Registry of multi-step wizards, mirroring `domains`. Adding a wizard is one
    /// line in `build_default_wizard_registry()`.
    pub(crate) wizards: Arc<WizardRegistry<AppState>>,
    /// Live state of the active wizard (cursor, history, accumulator).
    pub(crate) wizard_session: Mutex<WizardSession>,
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

    pub(crate) fn event_repo(&self) -> Arc<dyn EventRepository> {
        self.event_repo.clone()
    }

    pub(crate) fn god_repo(&self) -> Arc<dyn GodRepository> {
        self.god_repo.clone()
    }

    pub(crate) fn dungeon_repo(&self) -> Arc<dyn DungeonRepository> {
        self.dungeon_repo.clone()
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

    /// Resolve the current input context that gates autocomplete + help. Precedence:
    /// an open entity draft, then an active wizard, then the setup/config wizard,
    /// else the default surface. This is the single resolution point shared by the
    /// suggestion service and the desktop `help` handler so the two cannot drift
    /// (see docs/command-contexts.md).
    pub(crate) async fn resolve_input_context(&self) -> InputContext {
        let active_kind = {
            let editor = self.editor_session.lock().await;
            editor.active_kind()
        };
        if let Some(kind) = active_kind {
            return InputContext::EntityEditor(kind.as_str().to_string());
        }
        if let Some(id) = self.wizard_session.lock().await.active_id {
            return InputContext::Wizard(id.to_string());
        }
        let onboarding_active = {
            let service = self.command_service.lock().await;
            service.session().onboarding.active
        };
        if onboarding_active {
            InputContext::ConfigEditor
        } else {
            InputContext::Default
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // EditorSession is the state machine behind `system save/reroll/cancel`:
    // `active_kind` decides which draft those commands act on. A regression
    // here silently routes a save to the wrong entity or loses a draft, so
    // these lock the transition rules.

    fn npc_draft(name: &str) -> NpcDraftSession {
        NpcDraft {
            id: format!("npc-{name}"),
            seed_prompt: None,
            name: name.to_string(),
            slug: name.to_ascii_lowercase(),
            race: String::new(),
            occupation: String::new(),
            sex: String::new(),
            age: String::new(),
            height: String::new(),
            weight_lbs: String::new(),
            background: String::new(),
            want_need: String::new(),
            secret_obstacle: String::new(),
            carrying: Vec::new(),
            location: String::new(),
        }
    }

    fn location_draft(name: &str) -> LocationDraftSession {
        LocationDraft {
            id: format!("loc-{name}"),
            seed_prompt: None,
            name: name.to_string(),
            slug: name.to_ascii_lowercase(),
            vault_path: String::new(),
            kind_type: String::new(),
            kind_custom: None,
            visual_description: String::new(),
            history_background: String::new(),
            exports: Vec::new(),
            tone: String::new(),
            authority: String::new(),
            danger_level: String::new(),
            current_tension: String::new(),
        }
    }

    fn faction_draft(name: &str) -> FactionDraftSession {
        FactionDraft {
            id: format!("fac-{name}"),
            seed_prompt: None,
            name: name.to_string(),
            slug: name.to_ascii_lowercase(),
            vault_path: String::new(),
            kind_type: String::new(),
            kind_custom: None,
            public_description: String::new(),
            true_agenda: String::new(),
            methods: String::new(),
            leadership: String::new(),
            headquarters: String::new(),
            sphere_of_influence: String::new(),
            resources_assets: String::new(),
            allies: Vec::new(),
            rivals_enemies: Vec::new(),
            reputation: String::new(),
            current_tension: String::new(),
            goals_short_term: Vec::new(),
            goals_long_term: Vec::new(),
            symbol_description: String::new(),
        }
    }

    #[test]
    fn new_session_is_empty() {
        let session = EditorSession::default();
        assert_eq!(session.active_kind(), None);
        assert!(session.draft(EntityKind::Npc).is_none());
        assert!(session.get_npc().is_none());
    }

    #[test]
    fn setting_a_draft_activates_its_kind() {
        let mut session = EditorSession::default();
        session.set_npc(npc_draft("Lirael"));
        assert_eq!(session.active_kind(), Some(EntityKind::Npc));
        assert_eq!(session.get_npc().map(|d| d.name.as_str()), Some("Lirael"));
    }

    #[test]
    fn draft_envelope_reports_its_kind() {
        assert_eq!(DraftEnvelope::Npc(npc_draft("x")).kind(), EntityKind::Npc);
        assert_eq!(
            DraftEnvelope::Location(location_draft("y")).kind(),
            EntityKind::Location
        );
        assert_eq!(
            DraftEnvelope::Faction(faction_draft("z")).kind(),
            EntityKind::Faction
        );
    }

    #[test]
    fn opening_a_second_draft_switches_active_but_keeps_both() {
        let mut session = EditorSession::default();
        session.set_npc(npc_draft("Lirael"));
        session.set_location(location_draft("Harbor"));
        // Newest draft is active...
        assert_eq!(session.active_kind(), Some(EntityKind::Location));
        // ...but the npc draft is still retained, not clobbered.
        assert!(session.get_npc().is_some());
        assert!(session.get_location().is_some());
    }

    #[test]
    fn querying_a_kind_without_a_draft_returns_none() {
        let mut session = EditorSession::default();
        session.set_npc(npc_draft("Lirael"));
        // An NPC is open, but no faction draft exists.
        assert!(session.get_faction().is_none());
        assert!(session.draft(EntityKind::Faction).is_none());
    }

    #[test]
    fn activate_only_switches_to_a_kind_with_an_open_draft() {
        let mut session = EditorSession::default();
        session.set_npc(npc_draft("Lirael"));
        session.set_location(location_draft("Harbor"));
        session.activate(EntityKind::Npc);
        assert_eq!(session.active_kind(), Some(EntityKind::Npc));
        // No faction draft is open, so activate is a no-op.
        session.activate(EntityKind::Faction);
        assert_eq!(session.active_kind(), Some(EntityKind::Npc));
    }

    #[test]
    fn clearing_the_active_draft_falls_back_to_another_open_draft() {
        let mut session = EditorSession::default();
        session.set_npc(npc_draft("Lirael"));
        session.set_location(location_draft("Harbor"));
        assert_eq!(session.active_kind(), Some(EntityKind::Location));
        // Clearing the active (location) draft should not leave active_kind
        // dangling — it falls back to the still-open npc draft.
        let removed = session.clear_kind(EntityKind::Location);
        assert!(removed.is_some());
        assert_eq!(session.active_kind(), Some(EntityKind::Npc));
        assert!(session.get_location().is_none());
    }

    #[test]
    fn clearing_a_non_active_draft_leaves_active_kind_unchanged() {
        let mut session = EditorSession::default();
        session.set_npc(npc_draft("Lirael"));
        session.set_location(location_draft("Harbor"));
        // Location is active; clear the background npc draft.
        session.clear_kind(EntityKind::Npc);
        assert_eq!(session.active_kind(), Some(EntityKind::Location));
        assert!(session.get_npc().is_none());
    }

    #[test]
    fn clearing_the_last_draft_deactivates_the_editor() {
        let mut session = EditorSession::default();
        session.set_npc(npc_draft("Lirael"));
        session.clear_kind(EntityKind::Npc);
        assert_eq!(session.active_kind(), None);
    }

    #[test]
    fn take_returns_and_removes_the_draft() {
        let mut session = EditorSession::default();
        session.set_npc(npc_draft("Lirael"));
        let taken = session.take_npc();
        assert_eq!(taken.map(|d| d.name), Some("Lirael".to_string()));
        assert!(session.get_npc().is_none());
        assert_eq!(session.active_kind(), None);
    }

    #[test]
    fn clear_all_empties_the_session() {
        let mut session = EditorSession::default();
        session.set_npc(npc_draft("Lirael"));
        session.set_location(location_draft("Harbor"));
        session.set_faction(faction_draft("Syndicate"));
        session.clear_all();
        assert_eq!(session.active_kind(), None);
        assert!(session.get_npc().is_none());
        assert!(session.get_location().is_none());
        assert!(session.get_faction().is_none());
    }
}
