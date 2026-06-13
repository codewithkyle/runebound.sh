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

const TOP_LEVEL_COMMANDS = ["status", "config", "npc", "help"];
const CONFIG_SUBCOMMANDS = ["init", "show", "test", "doctor"];
const NPC_SUBCOMMANDS = ["create", "list", "show", "edit", "refs", "delete"];
const CONFIG_INIT_FLAGS = ["--vault-path", "--ollama-base-url", "--model", "--global", "--workspace", "--skip-test"];

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

  const clearCommand = () => {
    if (!command()) {
      return;
    }

    appendEntry("info", "^C");
    setCommand("");
    setSuggestionsDismissed(false);
    setActiveSuggestionIndex(0);
    inputRef?.focus();
  };

  const submitCommand = async () => {
    const raw = command().trim();
    if (!raw || running()) {
      return;
    }

    appendEntry("input", `> ${raw}`);
    setCommand("");
    setSuggestionsDismissed(false);
    setActiveSuggestionIndex(0);
    setRunning(true);

    try {
      const response = await invoke<CommandResponse>("run_command", { input: raw });
      if (response.ok) {
        appendEntry("output", response.output || "(ok)");
      } else {
        appendEntry("error", response.error || "command failed");
      }
    } catch (error) {
      appendEntry("error", `invoke error: ${String(error)}`);
    } finally {
      setRunning(false);
      inputRef?.focus();
    }
  };

  onMount(() => {
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
        setSuggestionsDismissed(false);
        setCommand((previous) => previous + event.key);
      }
    };

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => {
      window.removeEventListener("keydown", handleGlobalKeyDown);
    };
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
