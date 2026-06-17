//! The wizard framework: a first-class, registerable engine for multi-step flows,
//! mirroring the entity-domain architecture. Adding a wizard is additive data + one
//! trait impl; the plumbing (dispatch, nav verbs, clickable prompts, autocomplete
//! context, the spinner signal) lives here and never changes per wizard.
//!
//! See docs/create-wizard-refactor.md for the design and the entity↔wizard parallel.

pub mod dungeon;
pub mod prompt;
pub mod registry;
pub mod runtime;
pub mod session;
pub mod wizard;

pub use registry::{build_default_wizard_registry, WizardRegistry};
pub use runtime::{active_step_choices, start_wizard, try_execute_active_wizard};
pub use session::WizardSession;
pub use wizard::WizardChoice;
