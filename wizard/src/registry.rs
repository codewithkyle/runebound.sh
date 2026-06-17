//! The wizard registry: id -> `Arc<dyn Wizard<H>>`. Mirrors `EntityDomainRegistry`
//! so registering a wizard is the same one-line additive change as registering an
//! entity domain. Generic over the host type `H`; the concrete
//! `build_default_wizard_registry()` (which knows the app's wizards) lives in the
//! host crate, not here.

use std::collections::HashMap;
use std::sync::Arc;

use super::wizard::Wizard;

pub struct WizardRegistry<H: Send + Sync> {
    wizards: HashMap<&'static str, Arc<dyn Wizard<H>>>,
}

impl<H: Send + Sync> WizardRegistry<H> {
    pub fn new() -> Self {
        Self {
            wizards: HashMap::new(),
        }
    }

    pub fn register(&mut self, wizard: Arc<dyn Wizard<H>>) {
        self.wizards.insert(wizard.id(), wizard);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Wizard<H>>> {
        self.wizards.get(id).cloned()
    }
}

impl<H: Send + Sync> Default for WizardRegistry<H> {
    fn default() -> Self {
        Self::new()
    }
}
