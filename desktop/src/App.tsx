import { invoke } from "@tauri-apps/api/core";
import { For, Show, createEffect, createMemo, createSignal, onMount } from "solid-js";

type EntryKind = "input" | "output" | "error" | "info";

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

const TOP_LEVEL_COMMANDS = ["status", "config", "npc", "help", "clear", "history"];
const CONFIG_SUBCOMMANDS = ["init", "show", "test", "doctor"];
const NPC_SUBCOMMANDS = ["create", "list", "show", "edit", "refs", "delete"];
const CONFIG_INIT_FLAGS = ["--vault-path", "--ollama-base-url", "--model", "--global", "--workspace", "--skip-test"];
const CLEAR_FLAGS = ["--history"];
const HISTORY_SUBCOMMANDS = ["clear"];
const HISTORY_STORAGE_KEY = "dnd-assistant.command-history";
const MAX_COMMAND_HISTORY = 50;

export default function App() {
  const [entries, setEntries] = createSignal<HistoryEntry[]>([
    {
      id: 1,
      kind: "info",
      text: "DND Assistant ready."
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

  const runBuiltInCommand = (raw: string): { handled: boolean; ok: boolean; recordHistory: boolean } => {
    const tokens = raw.trim().split(/\s+/);
    const head = tokens[0]?.toLowerCase();
    if (!head) {
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

    const builtIn = runBuiltInCommand(raw);
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
            {(entry) => <pre class={entryClass(entry.kind)}>{entry.text}</pre>}
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
  return `${base} text-text`;
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
