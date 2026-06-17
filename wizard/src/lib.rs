//! The wizard framework: a first-class, registerable engine for multi-step flows,
//! mirroring the entity-domain architecture. Adding a wizard is additive data + one
//! trait impl; the plumbing (dispatch, nav verbs, clickable prompts, autocomplete
//! context, the spinner signal) lives here and never changes per wizard.
//!
//! The engine is host-agnostic: it is generic over a host type `H` (the
//! `WizardHost`) so the same engine drives the desktop app (`H = AppState`) and,
//! after the onboarding port, core/CLI. See docs/create-wizard-refactor.md for the
//! design and docs/onboarding-wizard-port.md for the cross-layer port.

pub mod prompt;
pub mod registry;
pub mod runtime;
pub mod session;
pub mod wizard;

use runebound_models::CommandResponse;

/// The handled/not-handled/failed contract every command path shares:
/// `Ok(Some(response))` handled, `Ok(None)` not handled (fall through), `Err`
/// failed. Mirrors the host's own `CommandResult` alias (same underlying type).
pub type CommandResult = Result<Option<CommandResponse>, String>;

pub use registry::WizardRegistry;
pub use runtime::{WizardHost, active_step_suggestions, start_wizard, try_execute_active_wizard};
pub use session::{WizardData, WizardSession};
pub use wizard::{
    NativeAction, NativeOutcome, Wizard, WizardChoice, WizardStep, WizardTransition,
};
