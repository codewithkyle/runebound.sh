//! The wizard registry: id -> `Arc<dyn Wizard>`. Mirrors `EntityDomainRegistry`
//! so registering a wizard is the same one-line additive change as registering an
//! entity domain.

use std::collections::HashMap;
use std::sync::Arc;

use super::dungeon::DungeonWizard;
use super::wizard::Wizard;

pub struct WizardRegistry {
    wizards: HashMap<&'static str, Arc<dyn Wizard>>,
}

impl WizardRegistry {
    pub fn new() -> Self {
        Self {
            wizards: HashMap::new(),
        }
    }

    pub fn register(&mut self, wizard: Arc<dyn Wizard>) {
        self.wizards.insert(wizard.id(), wizard);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Wizard>> {
        self.wizards.get(id).cloned()
    }
}

impl Default for WizardRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the registry with every wizard. Adding a wizard is one line here.
pub fn build_default_wizard_registry() -> WizardRegistry {
    let mut registry = WizardRegistry::new();
    registry.register(Arc::new(DungeonWizard::new()));
    registry
}
