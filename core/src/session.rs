use serde::{Deserialize, Serialize};

/// Which section(s) of setup an onboarding session is scoped to.
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

/// Sub-state of the vault step, used to disambiguate menu input from a typed path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VaultStepState {
    /// Not currently at the vault step.
    #[default]
    Inactive,
    /// Vault menu has been shown; awaiting a 1/2/3 choice.
    MenuShown,
    /// User chose "type a path"; awaiting the typed path.
    AwaitingPath,
}

/// Sub-state of the Ollama step, used to disambiguate menu input from a typed URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OllamaStepState {
    /// Not currently at the Ollama step.
    #[default]
    Inactive,
    /// Ollama menu has been shown; awaiting a 1/2 (or continue) choice.
    MenuShown,
    /// User chose "configure a new server"; awaiting the typed URL.
    AwaitingUrl,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingSession {
    pub active: bool,
    pub step: u8,
    pub flow: OnboardingFlow,
    pub vault_substate: VaultStepState,
    pub ollama_substate: OllamaStepState,
    pub vault_path: String,
    pub ollama_base_url: String,
    pub ollama_models: Vec<String>,
    pub selected_model: String,
}

impl Default for OnboardingSession {
    fn default() -> Self {
        Self {
            active: false,
            step: 0,
            flow: OnboardingFlow::Full,
            vault_substate: VaultStepState::Inactive,
            ollama_substate: OllamaStepState::Inactive,
            vault_path: String::new(),
            ollama_base_url: "http://127.0.0.1:11434".to_string(),
            ollama_models: Vec::new(),
            selected_model: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionState {
    pub command_history: Vec<String>,
    pub onboarding: OnboardingSession,
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
