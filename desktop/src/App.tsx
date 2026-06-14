import { invoke } from "@tauri-apps/api/core";
import { For, Show, createEffect, createMemo, createSignal, onMount } from "solid-js";
import { buildSuggestions as buildAutocompleteSuggestions, type SuggestionItem } from "./command/autocomplete";
import { loadManifest, parseInput, type CommandManifest, type ParseResult } from "./command/parser-client";
import {
  searchEntities,
  type EntitySuggestion,
} from "./entity/client";
import { parseOutputEntry } from "./output/markdown";
import { OutputRenderer } from "./output/renderer";
import type { OutputDoc } from "./output/types";

type EntryKind = "input" | "output" | "error" | "info" | "banner" | "spinner";

type HistoryEntry = {
  id: number;
  kind: EntryKind;
  text: string;
  outputDoc?: OutputDoc | null;
};

type CommandResponse = {
  ok: boolean;
  output: string;
  error?: string | null;
  exit_code: number;
  segments?: OutputSegment[];
  output_doc?: OutputDoc | null;
  client_event?: CommandClientEvent | null;
};

type CommandClientEvent =
  | {
      kind: "load_npc_draft";
      id: string;
      name: string;
      race: string;
      occupation: string;
      sex: string;
      age: string;
      height: string;
      weight_lbs: string;
      background: string;
      want_need: string;
      secret_obstacle: string;
      carrying: string[];
      location: string;
    }
  | {
      kind: "load_location_draft";
      id: string;
      name: string;
      slug: string;
      vault_path: string;
    }
  | {
      kind: "clear_drafts";
    };

type OutputSegment = {
  kind: "text" | "error";
  text: string;
  command_ref?: string | null;
};

type InlineCommandMeta = {
  commands: Set<string>;
  commandMap: Map<string, CommandSpecMeta>;
};

type CommandSpecMeta = {
  name: string;
  subcommands: Set<string>;
  requiresSubcommand: boolean;
  canonicalHelpCommand: string | null;
};

type NpcDraft = {
  id: string;
  name: string;
  race: string;
  occupation: string;
  sex: "male" | "female";
  age: string;
  height: string;
  weightLbs: string;
  background: string;
  wantNeed: string;
  secretObstacle: string;
  carrying: string[];
  location: string;
};

type LocationDraft = {
  id: string;
  name: string;
  slug: string;
  vault_path: string;
};

type SuggestionViewItem = SuggestionItem & {
  helperText?: "command" | "npc" | "location";
};

type EntitySuggestionItem = SuggestionViewItem & {
  helperText: "npc" | "location";
};

const SPINNER_FRAMES = ["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];

const HISTORY_STORAGE_KEY = "dnd-assistant.command-history";
const MAX_COMMAND_HISTORY = 50;

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
        "Type help to see available commands.\n",
      outputDoc: parseOutputEntry(
        "banner",
        "\n" +
          "╦═╗╦ ╦╔╗╔╔═╗╔╗ ╔═╗╦ ╦╔╗╔╔╦╗\n" +
          "╠╦╝║ ║║║║║╣ ╠╩╗║ ║║ ║║║║ ║║\n" +
          "╩╚═╚═╝╝╚╝╚═╝╚═╝╚═╝╚═╝╝╚╝═╩╝\n\n" +
          "\n" +
          "runebound.sh is an AI-assisted command console for game masters, lore keepers, and world builders.\n" +
          "\n" +
          "Type help to see available commands.\n",
        (candidate) => {
          if (candidate.trim().toLowerCase() === "help") {
            return "help";
          }
          return null;
        }
      )
    }
  ]);
  const [command, setCommand] = createSignal("");
  const [running, setRunning] = createSignal(false);
  const [activeSuggestionIndex, setActiveSuggestionIndex] = createSignal(0);
  const [suggestionsDismissed, setSuggestionsDismissed] = createSignal(false);
  const [commandHistory, setCommandHistory] = createSignal<string[]>([]);
  const [historyCursor, setHistoryCursor] = createSignal<number | null>(null);
  const [historyDraft, setHistoryDraft] = createSignal("");
  const [manifest, setManifest] = createSignal<CommandManifest | null>(null);
  const [parsedInput, setParsedInput] = createSignal<ParseResult | null>(null);
  const [editorMode, setEditorMode] = createSignal<"none" | "npc" | "location">("none");
  const [npcDraft, setNpcDraft] = createSignal<NpcDraft | null>(null);
  const [locationDraft, setLocationDraft] = createSignal<LocationDraft | null>(null);
  const [entitySuggestions, setEntitySuggestions] = createSignal<EntitySuggestionItem[]>([]);

  const commandMeta = createMemo(() => buildCommandMeta(manifest()));

  const suggestionList = createMemo(() => {
    if (command().trim().length === 0 || running() || suggestionsDismissed()) {
      return [] as SuggestionViewItem[];
    }

    const commandSuggestions: SuggestionViewItem[] = buildAutocompleteSuggestions(command(), manifest(), parsedInput()).map((item) => ({
      ...item,
      helperText: "command"
    }));
    const mode = editorMode();
    const filtered = commandSuggestions.filter((item) => {
      const completion = item.completion.trim().toLowerCase();
      const label = item.label.trim().toLowerCase();

      if (mode !== "npc") {
        if (completion === "npc" || completion.startsWith("npc ") || label === "npc" || label.startsWith("npc ")) {
          return false;
        }
        if (completion === "reroll" || label === "reroll") {
          return false;
        }
      }

      if (mode !== "location") {
        if (completion === "location" || completion.startsWith("location ") || label === "location" || label.startsWith("location ")) {
          return false;
        }
      }

      if (mode === "none" && (completion === "cancel" || label === "cancel")) {
        return false;
      }

      return true;
    });

    return [...filtered, ...entitySuggestions()];
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
  let parseGeneration = 0;
  let entitySuggestionGeneration = 0;

  createEffect(() => {
    const currentCommand = command();
    const loadedManifest = manifest();

    if (!loadedManifest) {
      setParsedInput(null);
      return;
    }

    const generation = parseGeneration + 1;
    parseGeneration = generation;

    void parseInput(currentCommand)
      .then((result) => {
        if (parseGeneration === generation) {
          setParsedInput(result);
        }
      })
      .catch(() => {
        if (parseGeneration === generation) {
          setParsedInput(null);
        }
      });
  });

  createEffect(() => {
    const raw = command();
    const query = raw.trim();
    const meta = commandMeta();
    const lowered = query.toLowerCase();

    const isLoadContext = lowered === "load" || lowered.startsWith("load ");
    const isDeleteContext = lowered === "delete" || lowered.startsWith("delete ");
    const searchQuery = isLoadContext
      ? query.slice(4).trim()
      : isDeleteContext
        ? query.slice(6).trim()
        : query;

    if (!query || (!isLoadContext && !isDeleteContext && startsWithKnownCommandRoot(query, meta))) {
      setEntitySuggestions([]);
      return;
    }

    if (!searchQuery) {
      setEntitySuggestions([]);
      return;
    }

    const generation = entitySuggestionGeneration + 1;
    entitySuggestionGeneration = generation;

    void searchEntities(searchQuery, 6)
      .then((results) => {
        if (entitySuggestionGeneration !== generation) {
          return;
        }

        const completionPrefix = isLoadContext ? "load" : isDeleteContext ? "delete" : null;
        setEntitySuggestions(results.map((result) => toEntitySuggestionItem(result, completionPrefix)));
      })
      .catch(() => {
        if (entitySuggestionGeneration === generation) {
          setEntitySuggestions([]);
        }
      });
  });

  const appendEntry = (kind: EntryKind, text: string, outputDoc?: OutputDoc | null) => {
    setEntries((prev) => [
      ...prev,
      {
        id: prev.length > 0 ? prev[prev.length - 1].id + 1 : 1,
        kind,
        text,
        outputDoc:
          kind === "input"
            ? null
            : outputDoc ?? parseOutputEntry(kind, text, (candidate) => resolveClickableCommandTarget(candidate, commandMeta()))
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

  const appendEntryWithId = (kind: EntryKind, text: string, outputDoc?: OutputDoc | null): number => {
    let nextId = 1;
    setEntries((prev) => {
      nextId = prev.length > 0 ? prev[prev.length - 1].id + 1 : 1;
      return [
        ...prev,
        {
          id: nextId,
          kind,
          text,
          outputDoc:
            kind === "input"
              ? null
              : outputDoc ?? parseOutputEntry(kind, text, (candidate) => resolveClickableCommandTarget(candidate, commandMeta()))
        }
      ];
    });
    queueMicrotask(() => {
      if (outputRef) {
        outputRef.scrollTo({
          top: outputRef.scrollHeight,
          behavior: "smooth"
        });
      }
    });
    return nextId;
  };

  const updateEntry = (id: number, kind: EntryKind, text: string, outputDoc?: OutputDoc | null) => {
    setEntries((prev) =>
      prev.map((entry) => {
        if (entry.id !== id) {
          return entry;
        }
        return {
          ...entry,
          kind,
          text,
          outputDoc:
            kind === "input"
              ? null
              : outputDoc ?? parseOutputEntry(kind, text, (candidate) => resolveClickableCommandTarget(candidate, commandMeta()))
        };
      })
    );
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
    const raw = rawInput;
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
      const rendered = responseToRenderableModel(response, commandMeta());
      if (response.ok) {
        applyClientEvent(response.client_event);
        appendEntry("output", rendered.text || "(ok)", rendered.outputDoc);
        pushCommandHistory(raw);
      } else {
        const errorText = response.error || rendered.text || "command failed";
        if (isBootstrapSetupMessage(errorText)) {
          appendEntry("output", errorText, rendered.outputDoc);
        } else {
          appendEntry("error", errorText, rendered.outputDoc);
        }
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

  const runStartupStatusCheck = async () => {
    if (running()) {
      return;
    }

    setRunning(true);
    try {
      const response = await invoke<CommandResponse>("run_command", { input: "status" });
      const rendered = responseToRenderableModel(response, commandMeta());
      if (response.ok) {
        applyClientEvent(response.client_event);
        appendEntry("output", rendered.text || "(ok)", rendered.outputDoc);
        return;
      }

      const errorText = response.error || rendered.text || "command failed";
      if (isBootstrapSetupMessage(errorText)) {
        appendEntry("output", errorText, rendered.outputDoc);
        appendEntry(
          "info",
          "bootstrap tip: run start setup, complete onboarding, then run status again."
        );
      } else {
        appendEntry("error", errorText);
      }
    } catch (error) {
      appendEntry("error", `startup check failed: ${String(error)}`);
    } finally {
      setRunning(false);
    }
  };

  const appendNpcSummary = (draft: NpcDraft) => {
    appendEntry("output", draft.name, npcDraftDoc(draft));
  };

  const appendLocationSummary = (draft: LocationDraft) => {
    appendEntry(
      "output",
      [
        "## Active Location draft",
        `name: ${draft.name}`,
        `slug: ${draft.slug}`,
        `path: ${draft.vault_path}`,
        "",
        "Use save to persist this location."
      ].join("\n")
    );
  };

  const applyClientEvent = (event: CommandClientEvent | null | undefined) => {
    if (!event) {
      return;
    }

    if (event.kind === "load_npc_draft") {
      const sex = event.sex.toLowerCase() === "female" ? "female" : "male";
      setLocationDraft(null);
      const draft: NpcDraft = {
        id: event.id,
        name: event.name,
        race: normalizeUnknown(event.race),
        occupation: normalizeUnknown(event.occupation),
        sex,
        age: normalizeUnknown(event.age),
        height: normalizeUnknown(event.height),
        weightLbs: normalizeUnknown(event.weight_lbs),
        background: normalizeUnknown(event.background),
        wantNeed: normalizeUnknown(event.want_need),
        secretObstacle: normalizeUnknown(event.secret_obstacle),
        carrying: normalizeUnknownList(event.carrying),
        location: normalizeUnknown(event.location)
      };
      setNpcDraft(draft);
      setEditorMode("npc");
      return;
    }

    if (event.kind === "clear_drafts") {
      setNpcDraft(null);
      setLocationDraft(null);
      setEditorMode("none");
      return;
    }

    setNpcDraft(null);
    const draft: LocationDraft = {
      id: event.id,
      name: event.name,
      slug: event.slug,
      vault_path: event.vault_path
    };
    setLocationDraft(draft);
    setEditorMode("location");
  };

  onMount(() => {
    void loadManifest()
      .then((loadedManifest) => {
        setManifest(loadedManifest);
      })
      .catch(() => {
        setManifest(null);
      });

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
    void runStartupStatusCheck();

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
                <Show
                  when={entry.kind !== "input"}
                  fallback={
                    <div>{entry.text}</div>
                  }
                >
                  <OutputRenderer
                    doc={entry.outputDoc as OutputDoc}
                    onRunCommand={(cmd) => {
                      void runDisplayedCommand(cmd);
                    }}
                  />
                </Show>
              </div>
            )}
          </For>
        </div>
      </main>

      <section class="shrink-0 pb-[2px]">
        <div class="w-full max-w-[960px] mx-auto">
          <div class="mb-[2px]">
            <Show when={suggestionList().length > 0}>
              <div class="bg-surface py-[2px]">
                <For each={suggestionList().slice(0, 8)}>
                  {(suggestion, index) => {
                    return (
                      <div
                        class="px-3 flex items-center justify-between gap-3"
                        classList={{
                          "text-accent": index() === activeSuggestionIndex(),
                          "bg-surface2": index() === activeSuggestionIndex(),
                          "text-text": index() !== activeSuggestionIndex()
                        }}
                      >
                        <span>{suggestion.label}</span>
                        <Show when={suggestion.helperText}>
                          <span class="text-muted text-xs">{suggestion.helperText}</span>
                        </Show>
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
                disabled={running()}
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

function segmentsToText(segments: OutputSegment[] | undefined, fallback: string): string {
  if (!segments || segments.length === 0) {
    return fallback;
  }

  return segments.map((segment) => segment.text).join("\n");
}

function responseToRenderableModel(response: CommandResponse, meta: InlineCommandMeta): { text: string; outputDoc: OutputDoc | null } {
  const text = segmentsToText(response.segments, response.output);
  return {
    text,
    outputDoc: response.output_doc ?? parseOutputEntry(response.ok ? "output" : "error", text, (candidate) => resolveClickableCommandTarget(candidate, meta))
  };
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
  if (kind === "spinner") {
    return `${base} text-text`;
  }
  return `${base} text-text`;
}

function resolveClickableCommandTarget(candidate: string, meta: InlineCommandMeta): string | null {
  const trimmed = candidate.trim();
  if (!trimmed) {
    return null;
  }

  if (isValidCommandLike(trimmed, meta)) {
    return trimmed;
  }

  const lowered = trimmed.toLowerCase();
  const command = meta.commandMap.get(lowered);
  if (command && command.requiresSubcommand && command.canonicalHelpCommand) {
    return command.canonicalHelpCommand;
  }

  return null;
}

function isValidCommandLike(input: string, meta: InlineCommandMeta): boolean {
  const trimmed = input.trim();
  if (!trimmed) {
    return false;
  }

  const tokens = trimmed.split(/\s+/);
  const lowered = tokens.map((token) => token.toLowerCase());
  const root = lowered[0];
  const command = meta.commandMap.get(root);
  if (!command) {
    return false;
  }

  if (lowered.length === 1 && !command.requiresSubcommand) {
    return true;
  }

  if (root === "history") {
    if (lowered.length === 2 && lowered[1] === "clear") {
      return true;
    }

    if (lowered.length === 2 && /^\d+$/.test(lowered[1])) {
      return true;
    }
  }

  if (root === "setup") {
    return lowered.length === 2 && lowered[1] === "help";
  }

  if (lowered.length >= 2 && command.subcommands.has(lowered[1])) {
    return true;
  }

  if (lowered.length >= 2 && lowered[1] === "--help") {
    return true;
  }

  if (root === "clear") {
    return lowered.length === 2 && lowered[1] === "--history";
  }

  return false;
}

function startsWithKnownCommandRoot(input: string, meta: InlineCommandMeta): boolean {
  const trimmed = input.trim();
  if (!trimmed) {
    return false;
  }

  const first = trimmed.split(/\s+/)[0].toLowerCase();
  return meta.commandMap.has(first);
}

function toEntitySuggestionItem(entity: EntitySuggestion, commandPrefix: "load" | "delete" | null): EntitySuggestionItem {
  const completion = commandPrefix ? `${commandPrefix} ${entity.name}` : entity.name;
  return {
    label: entity.name,
    completion,
    helperText: entity.entity_type
  };
}

function titleCaseSex(value: string): string {
  const lowered = value.toLowerCase();
  if (lowered === "male") {
    return "Male";
  }
  if (lowered === "female") {
    return "Female";
  }
  return value;
}

function normalizeUnknown(value: string | null | undefined): string {
  const normalized = (value ?? "").trim();
  if (!normalized) {
    return "Unknown";
  }
  return normalized;
}

function normalizeUnknownList(values: string[] | null | undefined): string[] {
  const cleaned = (values ?? []).map((value) => value.trim()).filter((value) => value.length > 0);
  if (cleaned.length === 0) {
    return ["Unknown"];
  }
  return cleaned;
}

function carryingToDisplay(values: string[] | null | undefined): string {
  return normalizeUnknownList(values).join(", ");
}

function npcDraftDoc(draft: NpcDraft): OutputDoc {
  return {
    blocks: [
      {
        kind: "entity_card",
        title: draft.name,
        rows: [
          { label: "Race:", value: normalizeUnknown(draft.race) },
          { label: "Occupation:", value: normalizeUnknown(draft.occupation) },
          { label: "Gender:", value: titleCaseSex(normalizeUnknown(draft.sex)) },
          { label: "Age:", value: normalizeUnknown(draft.age) },
          { label: "Height:", value: normalizeUnknown(draft.height) },
          { label: "Weight:", value: `${normalizeUnknown(draft.weightLbs)} lbs` },
          { label: "Background:", value: normalizeUnknown(draft.background) },
          { label: "Want:", value: normalizeUnknown(draft.wantNeed) },
          { label: "Secret:", value: normalizeUnknown(draft.secretObstacle) },
          { label: "Carrying:", value: carryingToDisplay(draft.carrying) },
          { label: "Location:", value: normalizeUnknown(draft.location) }
        ]
      },
      {
        kind: "paragraph",
        inlines: [
          { kind: "text", text: "Use " },
          { kind: "command_ref", label: "save", command: "save" },
          { kind: "text", text: " to persist this NPC, or " },
          { kind: "command_ref", label: "reroll", command: "reroll" },
          { kind: "text", text: " to generate again." }
        ]
      }
    ]
  };
}

function buildCommandMeta(manifest: CommandManifest | null): InlineCommandMeta {
  if (!manifest) {
    return {
      commands: new Set<string>(),
      commandMap: new Map<string, CommandSpecMeta>()
    };
  }

  const commandMap = new Map<string, CommandSpecMeta>();
  for (const command of manifest.commands) {
    const name = command.name.toLowerCase();
    commandMap.set(name, {
      name,
      subcommands: new Set(command.subcommands.map((subcommand) => subcommand.name.toLowerCase())),
      requiresSubcommand: command.requires_subcommand,
      canonicalHelpCommand: command.canonical_help_command ?? null
    });
  }

  return {
    commands: new Set([...commandMap.keys()]),
    commandMap
  };
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

function isBootstrapSetupMessage(message: string): boolean {
  return message.toLowerCase().includes("first-time setup required");
}
