import { invoke } from "@tauri-apps/api/core";
import { For, Show, createEffect, createMemo, createSignal, onMount } from "solid-js";

type EntryKind = "input" | "output" | "error" | "info" | "banner";

type HistoryEntry = {
  id: number;
  kind: EntryKind;
  text: string;
};

type CommandResponse = {
  ok: boolean;
  output: string;
  error?: string | null;
  exit_code: number;
};

type SuggestionItem = {
  label: string;
  completion: string;
};

type CommandMatch = {
  start: number;
  end: number;
  command: string;
};

const TOP_LEVEL_COMMANDS = ["status", "config", "npc", "help", "clear", "history", "exit"];
const CONFIG_SUBCOMMANDS = ["init", "show", "test", "doctor", "help"];
const NPC_SUBCOMMANDS = ["create", "list", "show", "edit", "refs", "delete", "help"];
const CONFIG_INIT_FLAGS = ["--vault-path", "--ollama-base-url", "--model", "--global", "--workspace", "--skip-test"];
const CLEAR_FLAGS = ["--history"];
const HISTORY_SUBCOMMANDS = ["clear"];
const HISTORY_STORAGE_KEY = "dnd-assistant.command-history";
const MAX_COMMAND_HISTORY = 50;
const SUBCOMMAND_ROOTS = ["config", "npc"];

export default function App() {
  const [entries, setEntries] = createSignal<HistoryEntry[]>([
    {
      id: 1,
      kind: "banner",
      text:
        "\n" +
        "╦═╗╦ ╦╔╗╔╔═╗╔╗ ╔═╗╦ ╦╔╗╔╔╦╗\n" +
        "╠╦╝║ ║║║║║╣ ╠╩╗║ ║║ ║║║║ ║║\n" +
        "╩╚═╚═╝╝╚╝╚═╝╚═╝╚═╝╚═╝╝╚╝═╩╝\n\n" +
        "\n" +
        "runebound.sh is an AI-assisted command console for game masters, lore keepers, and world builders.\n" +
        "\n" +
        "Type help to see available commands.\n"
    }
  ]);
  const [command, setCommand] = createSignal("");
  const [running, setRunning] = createSignal(false);
  const [activeSuggestionIndex, setActiveSuggestionIndex] = createSignal(0);
  const [suggestionsDismissed, setSuggestionsDismissed] = createSignal(false);
  const [commandHistory, setCommandHistory] = createSignal<string[]>([]);
  const [historyCursor, setHistoryCursor] = createSignal<number | null>(null);
  const [historyDraft, setHistoryDraft] = createSignal("");

  const suggestionList = createMemo(() => {
    if (command().trim().length === 0 || running() || suggestionsDismissed()) {
      return [] as SuggestionItem[];
    }
    return buildSuggestions(command());
  });

  createEffect(() => {
    const size = suggestionList().length;
    if (size === 0) {
      setActiveSuggestionIndex(0);
      return;
    }
    if (activeSuggestionIndex() >= size) {
      setActiveSuggestionIndex(0);
    }
  });

  let outputRef: HTMLDivElement | undefined;
  let inputRef: HTMLInputElement | undefined;

  const appendEntry = (kind: EntryKind, text: string) => {
    setEntries((prev) => [
      ...prev,
      {
        id: prev.length > 0 ? prev[prev.length - 1].id + 1 : 1,
        kind,
        text
      }
    ]);
    queueMicrotask(() => {
      if (outputRef) {
        outputRef.scrollTo({
          top: outputRef.scrollHeight,
          behavior: "smooth"
        });
      }
    });
  };

  const pushCommandHistory = (raw: string) => {
    const value = raw.trim();
    if (!value) {
      return;
    }

    setCommandHistory((previous) => {
      if (previous.length > 0 && previous[previous.length - 1] === value) {
        return previous;
      }

      const next = [...previous, value];
      if (next.length > MAX_COMMAND_HISTORY) {
        return next.slice(next.length - MAX_COMMAND_HISTORY);
      }
      return next;
    });
  };

  const resetHistoryNavigation = () => {
    setHistoryCursor(null);
    setHistoryDraft("");
  };

  const navigateHistoryUp = () => {
    const history = commandHistory();
    if (history.length === 0) {
      return;
    }

    if (historyCursor() === null) {
      setHistoryDraft(command());
      const idx = history.length - 1;
      setHistoryCursor(idx);
      setCommand(history[idx]);
      setActiveSuggestionIndex(0);
      return;
    }

    const current = historyCursor() as number;
    if (current > 0) {
      const nextIdx = current - 1;
      setHistoryCursor(nextIdx);
      setCommand(history[nextIdx]);
      setActiveSuggestionIndex(0);
    }
  };

  const navigateHistoryDown = () => {
    const history = commandHistory();
    const cursor = historyCursor();
    if (cursor === null || history.length === 0) {
      return;
    }

    if (cursor < history.length - 1) {
      const nextIdx = cursor + 1;
      setHistoryCursor(nextIdx);
      setCommand(history[nextIdx]);
      setActiveSuggestionIndex(0);
      return;
    }

    setHistoryCursor(null);
    setCommand(historyDraft());
    setHistoryDraft("");
    setActiveSuggestionIndex(0);
  };

  const historyOutput = (limit: number): string => {
    const history = commandHistory();
    if (history.length === 0) {
      return "(no history)";
    }

    const safeLimit = Math.max(1, Math.min(MAX_COMMAND_HISTORY, limit));
    const start = Math.max(0, history.length - safeLimit);
    return history
      .slice(start)
      .map((item, idx) => `${start + idx + 1}: ${item}`)
      .join("\n");
  };

  const parseHistoryExpansion = (raw: string): { ok: true; command: string } | { ok: false; error: string } | null => {
    const trimmed = raw.trim();
    if (trimmed === "!!") {
      const history = commandHistory();
      if (history.length === 0) {
        return { ok: false, error: "no command history available" };
      }
      return { ok: true, command: history[history.length - 1] };
    }

    const match = trimmed.match(/^!(\d+)$/);
    if (!match) {
      return null;
    }

    const index = Number.parseInt(match[1], 10);
    const history = commandHistory();
    if (Number.isNaN(index) || index < 1 || index > history.length) {
      return { ok: false, error: `history index out of range: ${index}` };
    }

    return { ok: true, command: history[index - 1] };
  };

  const runBuiltInCommand = async (raw: string): Promise<{ handled: boolean; ok: boolean; recordHistory: boolean }> => {
    const tokens = raw.trim().split(/\s+/);
    const head = tokens[0]?.toLowerCase();
    if (!head) {
      return { handled: true, ok: true, recordHistory: false };
    }

    if (head === "exit") {
      await invoke("exit_app");
      return { handled: true, ok: true, recordHistory: false };
    }

    if (head === "clear") {
      if (tokens.length === 1) {
        setEntries([]);
        return { handled: true, ok: true, recordHistory: true };
      }
      if (tokens.length === 2 && tokens[1] === "--history") {
        setEntries([]);
        setCommandHistory([]);
        resetHistoryNavigation();
        return { handled: true, ok: true, recordHistory: false };
      }

      appendEntry("error", "usage: clear [--history]");
      return { handled: true, ok: false, recordHistory: false };
    }

    if (head === "history") {
      if (tokens.length === 2 && tokens[1] === "clear") {
        setEntries([]);
        setCommandHistory([]);
        resetHistoryNavigation();
        return { handled: true, ok: true, recordHistory: false };
      }

      const limit = tokens.length > 1 ? Number.parseInt(tokens[1], 10) : 20;
      if (tokens.length > 1 && (Number.isNaN(limit) || limit < 1)) {
        appendEntry("error", "usage: history [limit|clear]");
        return { handled: true, ok: false, recordHistory: false };
      }
      appendEntry("output", historyOutput(limit));
      return { handled: true, ok: true, recordHistory: true };
    }

    return { handled: false, ok: false, recordHistory: false };
  };

  const executeCommand = async (rawInput: string) => {
    const expansion = parseHistoryExpansion(rawInput);
    if (expansion && !expansion.ok) {
      appendEntry("error", expansion.error);
      return;
    }

    const raw = expansion && expansion.ok ? expansion.command : rawInput;
    appendEntry("input", `> ${raw}`);

    const builtIn = await runBuiltInCommand(raw);
    if (builtIn.handled) {
      if (builtIn.ok && builtIn.recordHistory) {
        pushCommandHistory(raw);
      }
      return;
    }

    setRunning(true);
    try {
      const response = await invoke<CommandResponse>("run_command", { input: raw });
      if (response.ok) {
        appendEntry("output", response.output || "(ok)");
        pushCommandHistory(raw);
      } else {
        appendEntry("error", response.error || "command failed");
      }
    } catch (error) {
      appendEntry("error", `invoke error: ${String(error)}`);
    } finally {
      setRunning(false);
    }
  };

  const clearCommand = () => {
    if (!command()) {
      return;
    }

    setCommand("");
    resetHistoryNavigation();
    setSuggestionsDismissed(false);
    setActiveSuggestionIndex(0);
    inputRef?.focus();
  };

  const submitCommand = async () => {
    const raw = command().trim();
    if (!raw || running()) {
      return;
    }

    setCommand("");
    resetHistoryNavigation();
    setSuggestionsDismissed(false);
    setActiveSuggestionIndex(0);
    await executeCommand(raw);
    inputRef?.focus();
  };

  const runDisplayedCommand = async (raw: string) => {
    if (running()) {
      return;
    }

    setCommand("");
    resetHistoryNavigation();
    setSuggestionsDismissed(false);
    setActiveSuggestionIndex(0);
    await executeCommand(raw);
    inputRef?.focus();
  };

  onMount(() => {
    try {
      const serialized = window.localStorage.getItem(HISTORY_STORAGE_KEY);
      if (serialized) {
        const parsed = JSON.parse(serialized);
        if (Array.isArray(parsed)) {
          const cleaned = parsed
            .filter((item) => typeof item === "string")
            .map((item) => item.trim())
            .filter((item) => item.length > 0)
            .slice(-MAX_COMMAND_HISTORY);
          setCommandHistory(cleaned);
        }
      }
    } catch {
      setCommandHistory([]);
    }

    inputRef?.focus();

    const handleGlobalKeyDown = (event: KeyboardEvent) => {
      const ctrlOrMeta = event.ctrlKey || event.metaKey;

      if (ctrlOrMeta && event.key.toLowerCase() === "c") {
        if (command()) {
          event.preventDefault();
          clearCommand();
        }
        return;
      }

      if (isEditableTarget(event.target)) {
        return;
      }

      if (event.key === "Enter") {
        event.preventDefault();
        void submitCommand();
        return;
      }

      if (event.key.length === 1 && !event.altKey && !ctrlOrMeta) {
        event.preventDefault();
        inputRef?.focus();
        resetHistoryNavigation();
        setSuggestionsDismissed(false);
        setCommand((previous) => previous + event.key);
      }
    };

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => {
      window.removeEventListener("keydown", handleGlobalKeyDown);
    };
  });

  createEffect(() => {
    const history = commandHistory();
    try {
      window.localStorage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(history));
    } catch {
      // ignore write failures
    }
  });

  return (
    <div class="h-screen bg-bg text-text flex flex-col p-[8px]">
      <main ref={outputRef} class="flex-1 overflow-y-auto py-[2px]">
        <div class="w-full max-w-[960px] mx-auto space-y-2">
          <For each={entries()}>
            {(entry) => (
              <div class={entryClass(entry.kind)}>
                <For each={entry.text.split("\n")}>
                  {(line, lineIndex) => (
                    <Show
                      when={
                        entry.kind === "output" || entry.kind === "banner"
                          ? findClickableCommandInLine(line, inferUsagePrefix(entry.text))
                          : null
                      }
                      fallback={
                        <div class={bannerLineClass(entry.kind, lineIndex())}>{line.length === 0 ? "\u00A0" : line}</div>
                      }
                    >
                      {(match) => (
                        <div class={bannerLineClass(entry.kind, lineIndex())}>
                          <span>{line.slice(0, match().start)}</span>
                          <button
                            type="button"
                            class="text-info underline bg-transparent border-0 p-0 m-0 cursor-pointer"
                            onClick={() => {
                              void runDisplayedCommand(match().command);
                            }}
                          >
                            {displayClickableSegment(line.slice(match().start, match().end), match().command)}
                          </button>
                          <span>{line.slice(match().end)}</span>
                        </div>
                      )}
                    </Show>
                  )}
                </For>
              </div>
            )}
          </For>
        </div>
      </main>

      <section class="shrink-0 pb-[2px]">
        <div class="w-full max-w-[960px] mx-auto">
          <div class="mb-[2px]">
            <Show when={suggestionList().length > 0}>
              <div class="bg-surface px-3 py-[2px]">
                <For each={suggestionList().slice(0, 8)}>
                  {(suggestion, index) => {
                    return (
                      <div
                        classList={{
                          "text-accent": index() === activeSuggestionIndex(),
                          "bg-surface2": index() === activeSuggestionIndex(),
                          "text-text": index() !== activeSuggestionIndex()
                        }}
                      >
                        {suggestion.label}
                      </div>
                    );
                  }}
                </For>
              </div>
            </Show>
          </div>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void submitCommand();
            }}
          >
            <div class="w-full bg-surface2 px-3 py-[2px] flex items-center gap-2">
              <span class="text-accent">&gt;</span>
              <input
                ref={inputRef}
                class="w-full bg-transparent p-0 text-text focus:outline-none"
                type="text"
                value={command()}
                onInput={(event) => {
                  setCommand(event.currentTarget.value);
                  resetHistoryNavigation();
                  setSuggestionsDismissed(false);
                  setActiveSuggestionIndex(0);
                }}
                onKeyDown={(event) => {
                  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "c") {
                    if (command()) {
                      event.preventDefault();
                      clearCommand();
                    }
                    return;
                  }

                  if (event.key === "Escape") {
                    event.preventDefault();
                    setSuggestionsDismissed(true);
                    return;
                  }

                  const suggestions = suggestionList();
                  const shouldNavigateHistory = historyCursor() !== null || (command().trim().length === 0 && commandHistory().length > 0);

                  if (event.key === "ArrowUp" && shouldNavigateHistory) {
                    event.preventDefault();
                    navigateHistoryUp();
                    return;
                  }

                  if (event.key === "ArrowDown" && shouldNavigateHistory) {
                    event.preventDefault();
                    navigateHistoryDown();
                    return;
                  }

                  if (event.key === "ArrowDown" && suggestions.length > 1) {
                    event.preventDefault();
                    setActiveSuggestionIndex((previous) => (previous + 1) % suggestions.length);
                    return;
                  }

                  if (event.key === "ArrowUp" && suggestions.length > 1) {
                    event.preventDefault();
                    setActiveSuggestionIndex((previous) => (previous - 1 + suggestions.length) % suggestions.length);
                    return;
                  }

                  if (event.key === "Tab") {
                    event.preventDefault();

                    if (suggestions.length === 0) {
                      return;
                    }

                    const next = suggestions[Math.min(activeSuggestionIndex(), suggestions.length - 1)];
                    setCommand(next.completion);
                    setActiveSuggestionIndex(0);
                  }
                }}
              />
            </div>
          </form>
        </div>
      </section>
    </div>
  );
}

function entryClass(kind: EntryKind): string {
  const base = "whitespace-pre-wrap break-words";
  if (kind === "input") {
    return `${base} text-accent bg-surface2 px-3 py-[2px]`;
  }
  if (kind === "error") {
    return `${base} text-error`;
  }
  if (kind === "info") {
    return `${base} text-info`;
  }
  if (kind === "banner") {
    return `${base} text-text`;
  }
  return `${base} text-text`;
}

function findClickableCommandInLine(line: string, usagePrefix: string | null): CommandMatch | null {
  const backtickMatch = line.match(/`([^`]+)`/);
  if (backtickMatch) {
    const candidate = backtickMatch[1].trim();
    const commandTarget = resolveClickableCommandTarget(candidate);
    if (commandTarget) {
      const tickStart = line.indexOf(`\`${backtickMatch[1]}\``);
      if (tickStart >= 0) {
        return {
          start: tickStart,
          end: tickStart + backtickMatch[1].length + 2,
          command: commandTarget
        };
      }
    }
  }

  const historyMatch = line.match(/^(\s*\d+:\s+)(.+?)\s*$/);
  if (historyMatch) {
    const prefix = historyMatch[1];
    const candidate = historyMatch[2].trim();
    const commandTarget = resolveClickableCommandTarget(candidate);
    if (commandTarget) {
      const start = prefix.length;
      return {
        start,
        end: start + historyMatch[2].length,
        command: commandTarget
      };
    }
  }

  const usageMatch = line.match(/^(\s*Usage:\s+)(.+?)\s*$/i);
  if (usageMatch) {
    const prefix = usageMatch[1];
    const candidate = usageMatch[2].trim();
    const commandTarget = resolveClickableCommandTarget(candidate);
    if (commandTarget) {
      const start = prefix.length;
      return {
        start,
        end: start + usageMatch[2].length,
        command: commandTarget
      };
    }
  }

  const inlineTokenRegex = /\b(status|help|clear|history|config|npc|exit)\b/gi;
  let inlineMatch: RegExpExecArray | null;
  while ((inlineMatch = inlineTokenRegex.exec(line)) !== null) {
    const token = inlineMatch[1];
    const commandTarget = resolveClickableCommandTarget(token);
    if (commandTarget) {
      return {
        start: inlineMatch.index,
        end: inlineMatch.index + token.length,
        command: commandTarget
      };
    }
  }

  const commandTableMatch = line.match(/^(\s+)([a-z][a-z0-9-]*)(\s{2,}.*)?$/i);
  if (commandTableMatch) {
    const token = commandTableMatch[2].trim().toLowerCase();
    const tokenStart = line.indexOf(commandTableMatch[2]);
    if (tokenStart >= 0) {
      if (usagePrefix && isValidSubcommandForRoot(usagePrefix, token)) {
        return {
          start: tokenStart,
          end: tokenStart + commandTableMatch[2].length,
          command: `${usagePrefix} ${token}`
        };
      }

      const commandTarget = resolveClickableCommandTarget(token);
      if (commandTarget) {
        return {
          start: tokenStart,
          end: tokenStart + commandTableMatch[2].length,
          command: commandTarget
        };
      }
    }
  }

  const trimmed = line.trim();
  if (!trimmed) {
    return null;
  }

  const commandTarget = resolveClickableCommandTarget(trimmed);
  if (commandTarget) {
    const start = line.indexOf(trimmed);
    if (start >= 0) {
      return {
        start,
        end: start + trimmed.length,
        command: commandTarget
      };
    }
  }

  return null;
}

function resolveClickableCommandTarget(candidate: string): string | null {
  const trimmed = candidate.trim();
  if (!trimmed) {
    return null;
  }

  if (isValidCommandLike(trimmed)) {
    return trimmed;
  }

  const lowered = trimmed.toLowerCase();
  if (SUBCOMMAND_ROOTS.includes(lowered)) {
    return `${lowered} --help`;
  }

  return null;
}

function inferUsagePrefix(output: string): string | null {
  const usageLine = output
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line.toLowerCase().startsWith("usage:"));

  if (!usageLine) {
    return null;
  }

  const commandPart = usageLine.slice("usage:".length).trim();
  const firstToken = commandPart.split(/\s+/)[0]?.toLowerCase();
  if (!firstToken) {
    return null;
  }

  return TOP_LEVEL_COMMANDS.includes(firstToken) ? firstToken : null;
}

function isValidSubcommandForRoot(root: string, subcommand: string): boolean {
  if (root === "config") {
    return CONFIG_SUBCOMMANDS.includes(subcommand);
  }
  if (root === "npc") {
    return NPC_SUBCOMMANDS.includes(subcommand);
  }
  if (root === "history") {
    return HISTORY_SUBCOMMANDS.includes(subcommand);
  }
  return false;
}

function isValidCommandLike(input: string): boolean {
  const trimmed = input.trim();
  if (!trimmed) {
    return false;
  }

  if (trimmed === "!!") {
    return true;
  }
  if (/^!\d+$/.test(trimmed)) {
    return true;
  }

  const tokens = trimmed.split(/\s+/);
  const lowered = tokens.map((token) => token.toLowerCase());

  if (lowered.length === 1) {
    return ["status", "help", "clear", "history", "exit"].includes(lowered[0]);
  }

  if (lowered[0] === "clear") {
    return lowered.length === 2 && lowered[1] === "--history";
  }

  if (lowered[0] === "history") {
    if (lowered.length === 2 && lowered[1] === "clear") {
      return true;
    }
    return lowered.length === 2 && /^\d+$/.test(lowered[1]);
  }

  if (lowered[0] === "config") {
    if (lowered.length < 2) {
      return false;
    }
    const subcommand = lowered[1];
    if (subcommand === "init") {
      return true;
    }
    return ["show", "test", "doctor", "help"].includes(subcommand) && lowered.length === 2;
  }

  if (lowered[0] === "npc") {
    return lowered.length >= 2 && NPC_SUBCOMMANDS.includes(lowered[1]);
  }

  return false;
}

function buildSuggestions(input: string): SuggestionItem[] {
  const raw = input;
  const trimmed = raw.trim();
  const lowered = trimmed.toLowerCase();
  const endsWithSpace = raw.endsWith(" ");
  const tokens = trimmed.length === 0 ? [] : trimmed.split(/\s+/);
  const loweredTokens = lowered.length === 0 ? [] : lowered.split(/\s+/);

  if (tokens.length === 0) {
    return [];
  }

  if (tokens.length === 1 && !endsWithSpace) {
    const prefix = loweredTokens[0];
    return TOP_LEVEL_COMMANDS.filter((item) => item.startsWith(prefix)).map((label) => ({
      label,
      completion: `${label} `
    }));
  }

  const root = loweredTokens[0];
  if (root === "config") {
    return buildSubcommandSuggestions(raw, tokens, loweredTokens, endsWithSpace, CONFIG_SUBCOMMANDS, {
      init: CONFIG_INIT_FLAGS
    });
  }

  if (root === "npc") {
    return buildSubcommandSuggestions(raw, tokens, loweredTokens, endsWithSpace, NPC_SUBCOMMANDS, {});
  }

  if (root === "clear") {
    return buildFlagSuggestions(raw, tokens, endsWithSpace, CLEAR_FLAGS);
  }

  if (root === "history") {
    return buildSubcommandSuggestions(raw, tokens, loweredTokens, endsWithSpace, HISTORY_SUBCOMMANDS, {});
  }

  return [];
}

function buildFlagSuggestions(raw: string, tokens: string[], endsWithSpace: boolean, flags: string[]): SuggestionItem[] {
  if (tokens.length === 1 && endsWithSpace) {
    return flags.map((flag) => ({
      label: `${tokens[0]} ${flag}`,
      completion: `${tokens[0]} ${flag}`
    }));
  }

  if (tokens.length === 2 && !endsWithSpace) {
    const current = tokens[1].toLowerCase();
    return flags
      .filter((flag) => flag.startsWith(current))
      .map((flag) => ({
        label: `${tokens[0]} ${flag}`,
        completion: `${tokens[0]} ${flag}`
      }));
  }

  return [];
}

function buildSubcommandSuggestions(
  raw: string,
  tokens: string[],
  loweredTokens: string[],
  endsWithSpace: boolean,
  subcommands: string[],
  flagsBySubcommand: Record<string, string[]>
): SuggestionItem[] {
  if (tokens.length === 1 && endsWithSpace) {
    return subcommands.map((subcommand) => ({
      label: `${tokens[0]} ${subcommand}`,
      completion: `${tokens[0]} ${subcommand} `
    }));
  }

  if (tokens.length === 2 && !endsWithSpace) {
    const prefix = loweredTokens[1];
    return subcommands
      .filter((subcommand) => subcommand.startsWith(prefix))
      .map((subcommand) => ({
        label: `${tokens[0]} ${subcommand}`,
        completion: `${tokens[0]} ${subcommand} `
      }));
  }

  const subcommand = loweredTokens[1];
  const flags = flagsBySubcommand[subcommand] ?? [];
  if (flags.length === 0) {
    return [];
  }

  const currentToken = endsWithSpace ? "" : tokens[tokens.length - 1];
  const base = raw.slice(0, raw.length - currentToken.length);

  return flags
    .filter((flag) => flag.startsWith(currentToken))
    .map((flag) => ({
      label: `${tokens[0]} ${subcommand} ${flag}`,
      completion: `${base}${flag} `
    }));
}

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }

  if (target.isContentEditable) {
    return true;
  }

  return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;
}

function bannerLineClass(kind: EntryKind, lineIndex: number): string {
  if (kind === "banner" && lineIndex >= 1 && lineIndex <= 3) {
    return "text-[#8ec07c]";
  }
  return "";
}

function displayClickableSegment(rawSegment: string, command: string): string {
  const trimmed = rawSegment.trim();
  if (trimmed.startsWith("`") && trimmed.endsWith("`")) {
    return command;
  }
  return rawSegment;
}
