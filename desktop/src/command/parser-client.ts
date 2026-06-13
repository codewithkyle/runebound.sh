import { invoke } from "@tauri-apps/api/core";

export type CommandManifest = {
  commands: CommandSpec[];
  aliases: CommandAlias[];
};

export type CommandSpec = {
  name: string;
  summary: string;
  examples: string[];
  subcommands: SubcommandSpec[];
  options: OptionSpec[];
  requires_subcommand: boolean;
  canonical_help_command?: string | null;
  execution: "core" | "client";
  clap_managed: boolean;
  show_in_autocomplete: boolean;
};

export type SubcommandSpec = {
  name: string;
  summary: string;
  options: OptionSpec[];
  examples: string[];
  clap_managed: boolean;
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

export type ParseResult = {
  raw_input: string;
  raw_tokens: string[];
  normalized_tokens: string[];
  canonical_input: string;
  valid: boolean;
  diagnostics: ParseDiagnostic[];
  completion: CompletionContext;
};

export type ParseDiagnostic = {
  code: string;
  message: string;
};

export type CompletionContext = {
  stage: "root" | "subcommand" | "argument";
  root?: string | null;
  subcommand?: string | null;
  current_token: string;
  ends_with_space: boolean;
};

export async function loadManifest(): Promise<CommandManifest> {
  return invoke<CommandManifest>("get_command_manifest");
}

export async function parseInput(input: string): Promise<ParseResult> {
  return invoke<ParseResult>("parse_command_input", { input });
}
