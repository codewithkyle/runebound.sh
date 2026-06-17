//! The desktop's wizard layer: the host-specific glue over the shared `wizard`
//! engine crate. The engine (traits, session, registry, runtime, prompt helpers)
//! lives in the `wizard` crate, generic over a host type; here we bind it to the
//! desktop `AppState` and register the concrete wizards.
//!
//! Adding a wizard is additive: implement it under this module and register it in
//! `build_default_wizard_registry()`. The plumbing never changes per wizard.
//!
//! See docs/create-wizard-refactor.md for the design and the entity↔wizard parallel.

use std::sync::Arc;

use tokio::sync::Mutex;

use wizard::WizardHost;

use crate::app_state::AppState;

pub mod dungeon;

use dungeon::DungeonWizard;

// Re-export the engine surface used across the desktop crate so existing call
// sites (`crate::wizards::…`) keep working after the crate promotion.
pub use wizard::{
    WizardChoice, WizardRegistry, WizardSession, active_step_suggestions, start_wizard,
    try_execute_active_wizard,
};

/// Build the registry with every desktop wizard. Adding a wizard is one line here.
pub fn build_default_wizard_registry() -> WizardRegistry<AppState> {
    let mut registry = WizardRegistry::new();
    registry.register(Arc::new(DungeonWizard::new()));
    registry
}

/// Bind the shared engine to the desktop host: `AppState` owns the registry and
/// the live session, and is itself the context passed to steps' `accept()` /
/// `finalize()`.
impl WizardHost for AppState {
    fn wizard_registry(&self) -> &WizardRegistry<Self> {
        &self.wizards
    }

    fn wizard_session(&self) -> &Mutex<WizardSession> {
        &self.wizard_session
    }
}
