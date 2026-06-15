use async_trait::async_trait;

use crate::app_state::AppState;
use runebound_models::CommandResponse;

use super::kind::EntityKind;
use super::schema::EntitySchema;

pub type EntityDomainResult = Result<Option<CommandResponse>, String>;

#[async_trait]
pub trait EntityDomain: Send + Sync {
    fn kind(&self) -> EntityKind;
    #[allow(dead_code)]
    fn schema(&self) -> &'static EntitySchema;
    fn help_text(&self) -> String;

    async fn show_draft(&self, state: &AppState) -> EntityDomainResult;
    async fn rename(&self, value: &str, state: &AppState) -> EntityDomainResult;
    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> EntityDomainResult;
    async fn reroll_field(
        &self,
        field: &str,
        prompt: Option<String>,
        state: &AppState,
    ) -> EntityDomainResult;
    async fn save(&self, state: &AppState) -> EntityDomainResult;
    async fn cancel(&self, state: &AppState) -> EntityDomainResult;
}
