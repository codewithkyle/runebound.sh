//! The desktop's wizard layer: the host-specific glue over the shared `wizard`
//! engine crate. The engine (traits, session, registry, runtime, prompt helpers)
//! lives in the `wizard` crate, generic over a host type; here we bind it to the
//! desktop `AppState` and register the concrete wizards.
//!
//! Adding a wizard is additive: implement it under this module and register it in
//! `build_default_wizard_registry()`. The plumbing never changes per wizard.
//!
//! See docs/architecture.md §4 (Wizard Framework) and §8D for the design and the
//! entity↔wizard parallel.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use dnd_core::onboarding_wizard::{OnboardingHost, register_onboarding_wizards};
use wizard::{NativeAction, NativeOutcome, WizardHost};

use crate::app_state::AppState;
use crate::commands::setup_commands::{FolderPick, pick_vault_folder};

pub mod dungeon;
pub mod entity_link;
pub mod location;

use dungeon::DungeonWizard;
use location::LocationWizard;

// Re-export the engine surface used across the desktop crate so existing call
// sites (`crate::wizards::…`) keep working after the crate promotion.
pub use wizard::{
    WizardChoice, WizardRegistry, WizardSession, active_step_suggestions, start_wizard,
    try_execute_active_wizard,
};

/// Build the registry with every desktop wizard: the dungeon wizard plus the four
/// onboarding wizards (which run on `AppState` so the folder picker is available).
/// Adding a wizard is one line here.
pub fn build_default_wizard_registry() -> WizardRegistry<AppState> {
    let mut registry = WizardRegistry::new();
    registry.register(Arc::new(DungeonWizard::new()));
    registry.register(Arc::new(LocationWizard::new()));
    register_onboarding_wizards(&mut registry);
    registry
}

/// Bind the shared engine to the desktop host: `AppState` owns the registry and
/// the live session, and is itself the context passed to steps' `accept()` /
/// `finalize()`. `perform_native` opens the real folder picker for the onboarding
/// wizard's `PickFolder` action.
#[async_trait]
impl WizardHost for AppState {
    fn wizard_registry(&self) -> &WizardRegistry<Self> {
        &self.wizards
    }

    fn wizard_session(&self) -> &Mutex<WizardSession> {
        &self.wizard_session
    }

    async fn perform_native(&self, action: &NativeAction) -> NativeOutcome {
        match action {
            NativeAction::PickFolder { .. } => {
                // Clone the handle out (std mutex, not held across the dialog).
                let handle = self.app_handle.lock().unwrap().clone();
                let Some(handle) = handle else {
                    return NativeOutcome::Cancelled;
                };
                match pick_vault_folder(&handle) {
                    Ok(FolderPick::Picked(path)) => NativeOutcome::Provided(path),
                    Ok(FolderPick::Cancelled) | Err(_) => NativeOutcome::Cancelled,
                }
            }
        }
    }
}

/// Marks `AppState` as a host that can run the onboarding wizard on the desktop.
/// The only host capability beyond [`WizardHost`] is the native folder picker,
/// provided by `perform_native` above.
impl OnboardingHost for AppState {}
