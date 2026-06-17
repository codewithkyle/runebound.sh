//! Per-run wizard state: which wizard is active, where the cursor is, the history
//! stack that powers `back`, and the type-erased accumulator.
//!
//! Host-agnostic by design (no `AppState`) so the engine can move to a shared
//! crate later — only the trait `accept()`/`finalize()` touch host state.

use std::any::Any;

/// The per-wizard accumulator, type-erased so the engine is generic over every
/// wizard. Mirrors how `DraftEnvelope` erases entity drafts, but uses `Any` (not
/// an enum) so a future shared `wizard` crate need not know every concrete
/// wizard's data type.
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

/// Live state of the active wizard. `Default` is the inactive state (no wizard
/// running); the runtime resets to `default()` on cancel/complete.
#[derive(Default)]
pub struct WizardSession {
    /// `Some(id)` while a wizard is active; the `InputContext::Wizard` tag.
    pub active_id: Option<&'static str>,
    /// Index into the active wizard's `steps()`.
    pub cursor: usize,
    /// Stack of prior cursors, popped by `back`.
    pub history: Vec<usize>,
    /// The type-erased accumulator for the active wizard.
    pub data: Option<WizardData>,
}
