use async_trait::async_trait;

use crate::app_state::{AppState, DraftEnvelope};
use runebound_models::CommandResponse;

use super::kind::EntityKind;

pub type EntityDomainResult = Result<Option<CommandResponse>, String>;

/// A saved entity resolved from the store/db, in the typed draft form the editor
/// and the entity cards already consume. The kind-specific fields live in
/// `draft`; common fields are read through the accessors. (Soft-delete snapshots
/// the full DB row directly, so no extra metadata is carried here.)
#[derive(Debug, Clone)]
pub struct EntityDetail {
    pub draft: DraftEnvelope,
}

impl EntityDetail {
    pub fn kind(&self) -> EntityKind {
        self.draft.kind()
    }

    pub fn name(&self) -> &str {
        self.draft.name()
    }

    pub fn slug(&self) -> &str {
        self.draft.slug()
    }
}

#[async_trait]
pub trait EntityDomain: Send + Sync {
    fn kind(&self) -> EntityKind;
    fn help_text(&self) -> String;

    /// Look up a saved entity of this kind by name-or-slug, returning it as an
    /// [`EntityDetail`] (the typed draft + DB metadata), or `None` if this kind
    /// has no match. The registry loop in `EntityAdminService::resolve_entity`
    /// calls this for every kind.
    async fn resolve(
        &self,
        name_or_slug: &str,
        state: &AppState,
    ) -> Result<Option<EntityDetail>, String>;

    async fn show_draft(&self, state: &AppState) -> EntityDomainResult;
    async fn rename(&self, value: &str, state: &AppState) -> EntityDomainResult;
    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> EntityDomainResult;
    async fn reroll_field(
        &self,
        field: &str,
        prompt: Option<String>,
        state: &AppState,
    ) -> EntityDomainResult;

    /// Persist the active draft. Uniform across every kind, so the default body
    /// drives it from the registry/persistence layer via the draft's own type
    /// (see [`crate::entities::common::save_active_draft`]); domains do not override.
    async fn save(&self, state: &AppState) -> EntityDomainResult {
        crate::entities::common::save_active_draft(self.kind(), state).await
    }

    async fn cancel(&self, state: &AppState) -> EntityDomainResult;
}
