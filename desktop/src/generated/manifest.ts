// Auto-generated from the Rust types in `command-specs` + `services::suggestions`
// via ts-rs. Do not edit by hand. Regenerate with:
//   UPDATE_MODELS=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml

export type ValueHint = "path" | "url" | "model" | "integer" | "text";

export type CompletionHint = "none" | { "static_choices": Array<string> } | { "dynamic_provider": string };

export type CommandExecution = "core" | "desktop";

export type OptionSpec = { name: string, short: string | null, takes_value: boolean, value_hint: ValueHint | null, summary: string, completion: CompletionHint, };

export type SubcommandSpec = { name: string, summary: string, options: Array<OptionSpec>, examples: Array<string>, };

export type CommandSpec = { name: string, summary: string, examples: Array<string>, subcommands: Array<SubcommandSpec>, options: Array<OptionSpec>, requires_subcommand: boolean, canonical_help_command: string | null, execution: CommandExecution, show_in_autocomplete: boolean, };

export type SpinnerHint = { command: string, label: string, };

export type CommandAlias = { from: Array<string>, to: Array<string>, summary: string, };

export type CommandManifest = { commands: Array<CommandSpec>, aliases: Array<CommandAlias>, 
/**
 * Spinner/latency hints, so the frontend picks a progress label by looking up
 * the typed command instead of re-deriving the command taxonomy from input
 * strings. See [`spinner_hints`].
 */
spinner_hints: Array<SpinnerHint>, };

export type SuggestionHelperText = "command" | "npc" | "location" | "faction" | "item" | "event" | "god" | "dungeon" | "reference" | "spell";

export type CommandSuggestion = { label: string, completion: string, helper_text: SuggestionHelperText | null, };
