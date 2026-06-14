use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingSession {
    pub active: bool,
    pub step: u8,
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
