import type { CommandManifest, CommandSpec, ParseResult } from "./parser-client";

export type SuggestionItem = {
  label: string;
  completion: string;
};

export function buildSuggestions(input: string, manifest: CommandManifest | null, parsed: ParseResult | null): SuggestionItem[] {
  if (!manifest || !parsed || input.trim().length === 0) {
    return [];
  }

  if (parsed.completion.stage === "root") {
    return buildRootSuggestions(manifest, parsed.completion.current_token);
  }

  if (parsed.completion.stage === "subcommand") {
    return buildSubcommandSuggestions(manifest, parsed.completion.root ?? null, input, parsed.completion.current_token);
  }

  return buildArgumentSuggestions(manifest, parsed, input);
}

function buildRootSuggestions(manifest: CommandManifest, token: string): SuggestionItem[] {
  const prefix = token.toLowerCase();
  return manifest.commands
    .filter((cmd) => cmd.show_in_autocomplete)
    .filter((cmd) => cmd.name.startsWith(prefix))
    .map((cmd) => ({
      label: cmd.name,
      completion: `${cmd.name}${completionSuffix(cmd)}`
    }));
}

function buildSubcommandSuggestions(
  manifest: CommandManifest,
  root: string | null,
  input: string,
  token: string
): SuggestionItem[] {
  if (!root) {
    return [];
  }

  const command = findCommand(manifest, root);
  if (!command) {
    return [];
  }

  const prefix = token.toLowerCase();
  const base = replaceCurrentToken(input, token);

  return command.subcommands
    .filter((subcommand) => subcommand.name.startsWith(prefix))
    .map((subcommand) => ({
      label: `${command.name} ${subcommand.name}`,
      completion: `${base}${subcommand.name} `
    }));
}

function buildArgumentSuggestions(manifest: CommandManifest, parsed: ParseResult, input: string): SuggestionItem[] {
  const root = parsed.completion.root;
  if (!root) {
    return [];
  }

  const command = findCommand(manifest, root);
  if (!command) {
    return [];
  }

  const subcommand = parsed.completion.subcommand
    ? command.subcommands.find((item) => item.name === parsed.completion.subcommand)
    : undefined;

  if (command.name === "npc" && subcommand?.name === "travel") {
    const normalized = parsed.normalized_tokens.map((token) => token.toLowerCase());
    const hasTo = normalized.length >= 3 && normalized[2] === "to";
    if (!hasTo) {
      return [
        {
          label: "npc travel to",
          completion: "npc travel to "
        }
      ];
    }
  }

  if (command.name === "npc" && subcommand?.name === "set") {
    const fieldNames = ["name", "race", "sex", "age", "height", "weight", "background", "want", "secret", "carrying"];
    const args = parsed.normalized_tokens.slice(2);

    const shouldSuggestFields =
      args.length === 0 ||
      (args.length === 1 && !parsed.completion.ends_with_space);

    if (shouldSuggestFields) {
      const prefix = parsed.completion.current_token.toLowerCase();
      const base = replaceCurrentToken(input, parsed.completion.current_token);
      return fieldNames
        .filter((field) => field.startsWith(prefix))
        .map((field) => ({
          label: `npc set ${field}`,
          completion: `${base}${field} `
        }));
    }
  }

  if (command.name === "npc" && subcommand?.name === "reroll") {
    const fieldNames = ["name", "race", "sex", "age", "height", "weight", "background", "want", "secret", "carrying"];
    const args = parsed.normalized_tokens.slice(2);
    const shouldSuggestFields =
      args.length === 0 ||
      (args.length === 1 && !parsed.completion.ends_with_space);

    if (shouldSuggestFields) {
      const prefix = parsed.completion.current_token.toLowerCase();
      const base = replaceCurrentToken(input, parsed.completion.current_token);
      return fieldNames
        .filter((field) => field.startsWith(prefix))
        .map((field) => ({
          label: `npc reroll ${field}`,
          completion: `${base}${field} `
        }));
    }
  }

  const options = subcommand ? subcommand.options : command.options;
  if (options.length === 0) {
    return [];
  }

  const current = parsed.completion.current_token.toLowerCase();
  const used = new Set(parsed.normalized_tokens.filter((token) => token.startsWith("-")));
  const base = replaceCurrentToken(input, parsed.completion.current_token);
  const shouldFilterPrefix = current.startsWith("-") || current.length > 0;

  return options
    .filter((option) => !used.has(option.name) || option.takes_value)
    .filter((option) => !shouldFilterPrefix || option.name.startsWith(current))
    .map((option) => ({
      label: subcommand ? `${command.name} ${subcommand.name} ${option.name}` : `${command.name} ${option.name}`,
      completion: `${base}${option.name}${option.takes_value ? " " : ""}`
    }));
}

function findCommand(manifest: CommandManifest, root: string): CommandSpec | undefined {
  const normalized = root.toLowerCase();
  return manifest.commands.find((cmd) => cmd.name === normalized);
}

function replaceCurrentToken(input: string, currentToken: string): string {
  if (!currentToken) {
    return input;
  }

  return input.slice(0, Math.max(0, input.length - currentToken.length));
}

function completionSuffix(command: CommandSpec): string {
  if (command.subcommands.length > 0 || command.options.length > 0 || command.requires_subcommand) {
    return " ";
  }
  return "";
}
