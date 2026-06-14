import { invoke } from "@tauri-apps/api/core";
import { For, Show, createEffect, createMemo, createSignal, onMount } from "solid-js";
import { loadManifest, suggestInput, type CommandManifest, type CommandSuggestion } from "./command/parser-client";
import { parseOutputEntry } from "./output/markdown";
import { OutputRenderer } from "./output/renderer";
import type {
  OutputDoc,
  CommandClientEvent,
  CommandResponse,
  OutputSegment,
  NpcDraft,
  LocationDraft,
  FactionDraft,
} from "./generated/models";

type EntryKind = "input" | "output" | "error" | "info" | "banner" | "spinner";

type HistoryEntry = {
  id: number;
  kind: EntryKind;
  text: string;
  outputDoc?: OutputDoc | null;
};

type InlineCommandMeta = {
  commandMap: Map<string, CommandSpecMeta>;
};

type CommandSpecMeta = {
  subcommands: Set<string>;
  requiresSubcommand: boolean;
  canonicalHelpCommand: string | null;
};

type SuggestionViewItem = {
  label: string;
  completion: string;
  helperText?: "command" | "npc" | "location" | "faction" | "reference";
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
  const [editorMode, setEditorMode] = createSignal<"none" | "npc" | "location" | "faction">("none");
  const [npcDraft, setNpcDraft] = createSignal<NpcDraft | null>(null);
  const [locationDraft, setLocationDraft] = createSignal<LocationDraft | null>(null);
  const [factionDraft, setFactionDraft] = createSignal<FactionDraft | null>(null);
  const [suggestions, setSuggestions] = createSignal<SuggestionViewItem[]>([]);
  const [scrollbarCompensationPx, setScrollbarCompensationPx] = createSignal(0);

  const commandMeta = createMemo(() => buildCommandMeta(manifest()));

  const suggestionList = createMemo(() => suggestions());

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
  let inputRef: HTMLTextAreaElement | undefined;
  let suggestionGeneration = 0;

  const resizeCommandInput = () => {
    if (!inputRef) {
      return;
    }
    inputRef.style.height = "auto";
    inputRef.style.height = `${inputRef.scrollHeight}px`;
  };

  const handleCommandInputChange = (nextValue: string) => {
    setCommand(nextValue);
    resetHistoryNavigation();
    setSuggestionsDismissed(false);
    setActiveSuggestionIndex(0);
  };

  const insertCommandNewlineAtCursor = () => {
    if (!inputRef) {
      return;
    }
    const currentValue = command();
    const start = inputRef.selectionStart ?? currentValue.length;
    const end = inputRef.selectionEnd ?? currentValue.length;
    const nextValue = `${currentValue.slice(0, start)}\n${currentValue.slice(end)}`;
    handleCommandInputChange(nextValue);
    queueMicrotask(() => {
      if (!inputRef) {
        return;
      }
      const nextCursor = start + 1;
      inputRef.selectionStart = nextCursor;
      inputRef.selectionEnd = nextCursor;
      resizeCommandInput();
    });
  };

  const updateScrollbarCompensation = () => {
    if (!outputRef) {
      return;
    }
    const scrollbarWidth = Math.max(0, outputRef.offsetWidth - outputRef.clientWidth);
    setScrollbarCompensationPx(scrollbarWidth);
  };

  createEffect(() => {
    if (!manifest()) {
      setSuggestions([]);
      return;
    }

    if (running() || suggestionsDismissed()) {
      setSuggestions([]);
      return;
    }

    const currentCommand = command();
    if (currentCommand.trim().length === 0) {
      setSuggestions([]);
      return;
    }

    const generation = suggestionGeneration + 1;
    suggestionGeneration = generation;

    void suggestInput(currentCommand)
      .then((results) => {
        if (suggestionGeneration !== generation) {
          return;
        }
        setSuggestions(results.map(toSuggestionViewItem));
      })
      .catch(() => {
        if (suggestionGeneration === generation) {
          setSuggestions([]);
        }
      });
  });

  createEffect(() => {
    command();
    queueMicrotask(() => {
      resizeCommandInput();
    });
  });

  createEffect(() => {
    entries().length;
    queueMicrotask(() => {
      updateScrollbarCompensation();
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

  const executeCommand = async (rawInput: string) => {
    const raw = rawInput;
    appendEntry("input", `> ${raw}`);

    const spinnerLabel = commandSpinnerLabel(raw);
    const spinnerId = spinnerLabel ? appendEntryWithId("spinner", `${SPINNER_FRAMES[0]} ${spinnerLabel} ...`) : null;
    let spinnerFrame = 0;
    const spinnerTimer = spinnerId
      ? window.setInterval(() => {
          spinnerFrame = (spinnerFrame + 1) % SPINNER_FRAMES.length;
          updateEntry(spinnerId, "spinner", `${SPINNER_FRAMES[spinnerFrame]} ${spinnerLabel} ...`);
        }, 100)
      : null;

    setRunning(true);
    try {
      const response = await invoke<CommandResponse>("run_command", { input: raw });
      const rendered = responseToRenderableModel(response, commandMeta());
      if (response.ok) {
        if (spinnerId !== null) {
          updateEntry(spinnerId, "spinner", `OK ${spinnerLabel}`);
        }
        applyClientEvent(response.client_event);
        const outputDocOverride = outputDocFromClientEvent(response.client_event);
        const suppressOutput =
          response.client_event?.kind === "clear_terminal" && rendered.text.trim().length === 0;
        if (!suppressOutput) {
          appendEntry("output", rendered.text || "(ok)", outputDocOverride ?? rendered.outputDoc);
        }
        pushCommandHistory(raw);
      } else {
        if (spinnerId !== null) {
          updateEntry(spinnerId, "spinner", `FAILED ${spinnerLabel}`);
        }
        const errorText = response.error || rendered.text || "command failed";
        if (isBootstrapSetupMessage(errorText)) {
          appendEntry("output", errorText, rendered.outputDoc);
        } else {
          appendEntry("error", errorText, rendered.outputDoc);
        }
      }
    } catch (error) {
      if (spinnerId !== null) {
        updateEntry(spinnerId, "spinner", `FAILED ${spinnerLabel}`);
      }
      appendEntry("error", `invoke error: ${String(error)}`);
    } finally {
      if (spinnerTimer !== null) {
        window.clearInterval(spinnerTimer);
      }
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
    const raw = normalizeSubmittedCommand(command());
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
      } else {
        appendEntry("error", errorText);
      }
    } catch (error) {
      appendEntry("error", `startup check failed: ${String(error)}`);
    } finally {
      setRunning(false);
    }
  };

  const applyClientEvent = (event: CommandClientEvent | null | undefined) => {
    if (!event) {
      return;
    }

    switch (event.kind) {
      case "load_npc_draft_with_card":
        setNpcDraft(event.draft);
        setLocationDraft(null);
        setFactionDraft(null);
        setEditorMode("npc");
        return;
      case "load_location_draft_with_card":
        setLocationDraft(event.draft);
        setNpcDraft(null);
        setFactionDraft(null);
        setEditorMode("location");
        return;
      case "load_faction_draft_with_card":
        setFactionDraft(event.draft);
        setNpcDraft(null);
        setLocationDraft(null);
        setEditorMode("faction");
        return;
      case "load_npc_draft":
      case "load_location_draft":
      case "load_faction_draft":
        console.warn("Received legacy draft event without entity card", event);
        return;
      case "clear_drafts":
        setNpcDraft(null);
        setLocationDraft(null);
        setFactionDraft(null);
        setEditorMode("none");
        return;
      case "clear_terminal":
        setEntries([]);
        if (event.clear_history) {
          setCommandHistory([]);
          resetHistoryNavigation();
        }
        return;
      case "exit_requested":
        void invoke("exit_app");
        return;
      default: {
        const exhaustiveCheck: never = event;
        return exhaustiveCheck;
      }
    }
  };

  const outputDocFromClientEvent = (event: CommandClientEvent | null | undefined): OutputDoc | null => {
    if (!event) {
      return null;
    }

    switch (event.kind) {
      case "load_npc_draft_with_card":
      case "load_location_draft_with_card":
      case "load_faction_draft_with_card":
        return event.entity_card;
      case "load_npc_draft":
      case "load_location_draft":
      case "load_faction_draft":
        console.warn("Legacy client event is missing entity_card", event);
        return null;
      case "clear_drafts":
      case "clear_terminal":
      case "exit_requested":
        return null;
      default: {
        const exhaustiveCheck: never = event;
        return exhaustiveCheck;
      }
    }
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
    resizeCommandInput();
    updateScrollbarCompensation();
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

    const handleWindowResize = () => {
      updateScrollbarCompensation();
    };

    let resizeObserver: ResizeObserver | undefined;
    if (typeof ResizeObserver !== "undefined" && outputRef) {
      resizeObserver = new ResizeObserver(() => {
        updateScrollbarCompensation();
      });
      resizeObserver.observe(outputRef);
    }

    window.addEventListener("keydown", handleGlobalKeyDown);
    window.addEventListener("resize", handleWindowResize);
    return () => {
      window.removeEventListener("keydown", handleGlobalKeyDown);
      window.removeEventListener("resize", handleWindowResize);
      resizeObserver?.disconnect();
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
      <main ref={outputRef} class="rb-output-scroll flex-1 overflow-y-auto py-[2px]">
        <div class="w-full max-w-[1040px] mx-auto space-y-2">
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
        <div
          class="w-full max-w-[1040px] mx-auto"
          style={{
            "padding-right": `${scrollbarCompensationPx()}px`
          }}
        >
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
            <div class="w-full bg-surface2 px-3 py-[2px] flex items-start gap-2">
              <span class="text-accent pt-[1px]">&gt;</span>
              <textarea
                ref={inputRef}
                class="w-full bg-transparent p-0 text-text focus:outline-none resize-none overflow-hidden whitespace-pre-wrap break-words"
                disabled={running()}
                rows={1}
                value={command()}
                onInput={(event) => {
                  handleCommandInputChange(event.currentTarget.value);
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

                  if (event.key.toLowerCase() === "j" && event.ctrlKey && !event.metaKey && !event.altKey) {
                    event.preventDefault();
                    insertCommandNewlineAtCursor();
                    return;
                  }

                  if (event.key === "Enter" && !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
                    event.preventDefault();
                    void submitCommand();
                    return;
                  }

                  const suggestions = suggestionList();
                  const caretStart = event.currentTarget.selectionStart ?? 0;
                  const caretEnd = event.currentTarget.selectionEnd ?? 0;
                  const atTextStart = caretStart === 0 && caretEnd === 0;
                  const atTextEnd =
                    caretStart === event.currentTarget.value.length &&
                    caretEnd === event.currentTarget.value.length;
                  const shouldNavigateHistoryUp =
                    atTextStart &&
                    (historyCursor() !== null || (command().trim().length === 0 && commandHistory().length > 0));
                  const shouldNavigateHistoryDown =
                    atTextEnd &&
                    (historyCursor() !== null || (command().trim().length === 0 && commandHistory().length > 0));
                  const allowSuggestionArrowNav = !command().includes("\n");

                  if (event.key === "ArrowUp" && shouldNavigateHistoryUp) {
                    event.preventDefault();
                    navigateHistoryUp();
                    return;
                  }

                  if (event.key === "ArrowDown" && shouldNavigateHistoryDown) {
                    event.preventDefault();
                    navigateHistoryDown();
                    return;
                  }

                  if (allowSuggestionArrowNav && event.key === "ArrowDown" && suggestions.length > 1) {
                    event.preventDefault();
                    setActiveSuggestionIndex((previous) => (previous + 1) % suggestions.length);
                    return;
                  }

                  if (allowSuggestionArrowNav && event.key === "ArrowUp" && suggestions.length > 1) {
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

function toSuggestionViewItem(suggestion: CommandSuggestion): SuggestionViewItem {
  return {
    label: suggestion.label,
    completion: suggestion.completion,
    helperText: suggestion.helper_text ?? undefined
  };
}

function normalizeSubmittedCommand(value: string): string {
  return value.replace(/\r?\n/g, " ").trim();
}
function buildCommandMeta(manifest: CommandManifest | null): InlineCommandMeta {
  if (!manifest) {
    return {
      commandMap: new Map<string, CommandSpecMeta>()
    };
  }

  const commandMap = new Map<string, CommandSpecMeta>();
  for (const command of manifest.commands) {
    commandMap.set(command.name.toLowerCase(), {
      subcommands: new Set(command.subcommands.map((subcommand) => subcommand.name.toLowerCase())),
      requiresSubcommand: command.requires_subcommand,
      canonicalHelpCommand: command.canonical_help_command ?? null
    });
  }

  return {
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

function commandSpinnerLabel(raw: string): string | null {
  const lowered = raw.trim().toLowerCase();
  if (lowered === "create npc" || lowered.startsWith("create npc ")) {
    return "generating npc";
  }
  if (lowered === "create location" || lowered.startsWith("create location ")) {
    return "generating location";
  }
  if (lowered === "create faction" || lowered.startsWith("create faction ")) {
    return "generating faction";
  }
  if (lowered === "reroll" || lowered === "npc reroll" || lowered.startsWith("npc reroll ")) {
    return "rerolling npc";
  }
  if (lowered === "location reroll" || lowered.startsWith("location reroll ")) {
    return "rerolling location";
  }
  if (lowered === "faction reroll" || lowered.startsWith("faction reroll ")) {
    return "rerolling faction";
  }
  if (lowered.startsWith("npc save") || lowered.startsWith("location save") || lowered === "save") {
    return "saving draft";
  }
  if (lowered.startsWith("faction save")) {
    return "saving draft";
  }
  return null;
}
