import { invoke } from "@tauri-apps/api/core";

export type SetupState = {
  needs_setup: boolean;
  issues: string[];
  global_config_path: string;
  workspace_config_path: string;
  default_ollama_base_url: string;
};

export type OllamaProbeResult = {
  ok: boolean;
  detail: string;
  models: string[];
};

export type SetupScope = "global" | "workspace" | "auto";

export type SaveOnboardingInput = {
  vault_path: string;
  ollama_base_url: string;
  model: string;
  scope: SetupScope;
};

export type SaveOnboardingResult = {
  config_path: string;
  vault_path: string;
  db_path: string;
  warnings: string[];
};

export async function getSetupState(): Promise<SetupState> {
  return invoke<SetupState>("get_setup_state");
}

export async function validateVaultPath(path: string): Promise<void> {
  return invoke("validate_vault_path", { path });
}

export async function probeOllama(baseUrl: string, timeoutSeconds = 15): Promise<OllamaProbeResult> {
  return invoke<OllamaProbeResult>("probe_ollama", {
    baseUrl,
    timeoutSeconds
  });
}

export async function saveOnboardingConfig(input: SaveOnboardingInput): Promise<SaveOnboardingResult> {
  return invoke<SaveOnboardingResult>("save_onboarding_config", { input });
}
