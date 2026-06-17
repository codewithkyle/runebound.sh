use serde::{Deserialize, Serialize};

/// Which section(s) of setup an onboarding wizard run is scoped to. Used by the
/// onboarding wizards (`core/src/onboarding_wizard.rs`) to pick per-flow step
/// transitions and the config section each `finalize` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingFlow {
    /// `start setup` — vault → ollama url → model → save.
    #[default]
    Full,
    /// `setup vault` — vault only; saves that section and exits.
    Vault,
    /// `setup llm` — ollama url + model only; saves that section and exits.
    Llm,
    /// `setup model` / `model` — pick a model on the existing server; saves and exits.
    Model,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionState {
    pub command_history: Vec<String>,
}

impl SessionState {
    pub fn push_history(&mut self, command: &str, max_items: usize) {
        let value = command.trim();
        if value.is_empty() {
            return;
        }

        if self
            .command_history
            .last()
            .is_some_and(|last| last == value)
        {
            return;
        }

        self.command_history.push(value.to_string());
        if self.command_history.len() > max_items {
            let trim_to = self.command_history.len() - max_items;
            self.command_history.drain(0..trim_to);
        }
    }

    pub fn clear_history(&mut self) {
        self.command_history.clear();
    }
}
