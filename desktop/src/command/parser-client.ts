import { invoke } from "@tauri-apps/api/core";

export type CommandManifest = {
  commands: CommandSpec[];
  aliases: CommandAlias[];
  spinner_hints: SpinnerHint[];
};

export type SpinnerHint = {
  command: string;
  label: string;
};

export type CommandSpec = {
  name: string;
  summary: string;
  examples: string[];
  subcommands: SubcommandSpec[];
  options: OptionSpec[];
  requires_subcommand: boolean;
  canonical_help_command?: string | null;
  execution: "core" | "desktop";
  show_in_autocomplete: boolean;
};

export type SubcommandSpec = {
  name: string;
  summary: string;
  options: OptionSpec[];
  examples: string[];
};

export type OptionSpec = {
  name: string;
  short?: string | null;
  takes_value: boolean;
  value_hint?: "path" | "url" | "model" | "integer" | "text" | null;
  summary: string;
  completion: { static_choices?: string[] } | "none" | { dynamic_provider: string };
};

export type CommandAlias = {
  from: string[];
  to: string[];
  summary: string;
};

export type SuggestionHelperText = "command" | "npc" | "location" | "faction" | "item" | "event" | "god" | "reference";

export type CommandSuggestion = {
  label: string;
  completion: string;
  helper_text?: SuggestionHelperText | null;
};

export async function loadManifest(): Promise<CommandManifest> {
  return invoke<CommandManifest>("get_command_manifest");
}

export async function suggestInput(input: string): Promise<CommandSuggestion[]> {
  return invoke<CommandSuggestion[]>("suggest_command_input", { input });
}
