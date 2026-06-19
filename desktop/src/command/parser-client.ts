import { invoke } from "@tauri-apps/api/core";

// The manifest + suggestion types are generated from the Rust definitions
// (`command-specs` + `services::suggestions`) via ts-rs. Re-exported here so the
// command API and its types live behind one module. Regenerate with:
//   UPDATE_MODELS=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml
export type {
  CommandManifest,
  CommandSpec,
  SubcommandSpec,
  OptionSpec,
  CommandAlias,
  SpinnerHint,
  ValueHint,
  CompletionHint,
  CommandExecution,
  CommandSuggestion,
  SuggestionHelperText,
} from "../generated/manifest";

import type { CommandManifest, CommandSuggestion } from "../generated/manifest";

export async function loadManifest(): Promise<CommandManifest> {
  return invoke<CommandManifest>("get_command_manifest");
}

export async function suggestInput(input: string): Promise<CommandSuggestion[]> {
  return invoke<CommandSuggestion[]>("suggest_command_input", { input });
}
