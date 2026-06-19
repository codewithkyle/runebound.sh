//! Per-run wizard state: which wizard is active, where the cursor is, the history
//! stack that powers `back`, and the type-erased accumulator.
//!
//! Host-agnostic by design (no host type) — only the trait `accept()`/`finalize()`
//! touch host state.

use std::any::Any;

/// The per-wizard accumulator, type-erased so the engine is generic over every
/// wizard. Mirrors how `DraftEnvelope` erases entity drafts, but uses `Any` (not
/// an enum) so the shared `wizard` crate need not know every concrete wizard's
/// data type.
///
/// The `downcast_*` accessors return `Option`, but the concrete type is fixed at
/// `seed()` time and never changes during a run, so a `None` means a step asked
/// for the wrong type — a *construction bug* in that wizard, not a runtime
/// condition. Step/host code may therefore `expect` the downcast.
pub struct WizardData(Box<dyn Any + Send + Sync>);

impl WizardData {
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        WizardData(Box::new(value))
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }

    pub fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.0.downcast_mut::<T>()
    }
}

/// Live state of the wizard engine. Modeled as a two-state enum (P7.4) so "a
/// wizard is running" and "its accumulator exists" are a single fact rather than a
/// cross-field invariant the runtime has to assert with `expect`: an [`Active`]
/// session *always* carries its [`ActiveWizard::data`]. `Default` is [`Inactive`];
/// the runtime resets to it on cancel/complete.
///
/// [`Active`]: WizardSession::Active
/// [`Inactive`]: WizardSession::Inactive
#[derive(Default)]
pub enum WizardSession {
    /// No wizard running — the `InputContext::Default` state.
    #[default]
    Inactive,
    /// A wizard is mid-run, with its cursor, history, and accumulator.
    Active(ActiveWizard),
}

/// The state of an in-progress wizard run (the `Active` variant's payload).
pub struct ActiveWizard {
    /// The active wizard's id — the `InputContext::Wizard` tag.
    pub id: &'static str,
    /// Index into the active wizard's `steps()`.
    pub cursor: usize,
    /// Stack of prior cursors, popped by `back`.
    pub history: Vec<usize>,
    /// The type-erased accumulator for this run.
    pub data: WizardData,
}

impl WizardSession {
    /// The active wizard's id, or `None` when inactive. The one piece of session
    /// state read across module boundaries (hosts gate `InputContext` on it).
    pub fn active_id(&self) -> Option<&'static str> {
        match self {
            WizardSession::Active(active) => Some(active.id),
            WizardSession::Inactive => None,
        }
    }
}
