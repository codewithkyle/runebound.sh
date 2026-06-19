import { invoke } from "@tauri-apps/api/core";
import { For, Show, createEffect, createMemo, createSignal, onMount } from "solid-js";
import { loadManifest, suggestInput, type CommandManifest, type CommandSuggestion, type SuggestionHelperText } from "./command/parser-client";
import { buildEntryDoc } from "./output/entry-doc";
import { OutputRenderer } from "./output/renderer";
import type {
  OutputDoc,
  CommandClientEvent,
  CommandResponse,
  OutputSegment,
  WizardView,
} from "./generated/models";

type EntryKind = "input" | "output" | "error" | "info" | "banner" | "spinner";

type HistoryEntry = {
  id: number;
  kind: EntryKind;
  text: string;
  outputDoc?: OutputDoc | null;
};

type SuggestionViewItem = {
  label: string;
  completion: string;
  helperText?: SuggestionHelperText;
};

const SPINNER_FRAMES = ["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];

// Minimum time a boot spinner stays on screen, even if its task finishes
// instantly, so the boot sequence reads as deliberate rather than flickering.
const BOOT_SPINNER_MIN_MS = 300;

const BANNER_TEXT =
  "\n" +
  "╦═╗╦ ╦╔╗╔╔═╗╔╗ ╔═╗╦ ╦╔╗╔╔╦╗\n" +
  "╠╦╝║ ║║║║║╣ ╠╩╗║ ║║ ║║║║ ║║\n" +
  "╩╚═╚═╝╝╚╝╚═╝╚═╝╚═╝╚═╝╝╚╝═╩╝\n\n" +
  "\n" +
  "runebound.sh is an AI-assisted command console for game masters, lore keepers, and world builders.\n" +
  "\n" +
  "Type help to see available commands.\n";

// The banner is authored as a structured doc (not parsed from BANNER_TEXT): the
// ASCII art is a code block, and `help` is a real command_ref. Clickability never
// comes from guessing the rendered text.
const BANNER_DOC: OutputDoc = {
  blocks: [
    {
      kind: "code",
      language: null,
      text:
        "╦═╗╦ ╦╔╗╔╔═╗╔╗ ╔═╗╦ ╦╔╗╔╔╦╗\n" +
        "╠╦╝║ ║║║║║╣ ╠╩╗║ ║║ ║║║║ ║║\n" +
        "╩╚═╚═╝╝╚╝╚═╝╚═╝╚═╝╚═╝╝╚╝═╩╝"
    },
    {
      kind: "paragraph",
      inlines: [
        {
          kind: "text",
          text: "runebound.sh is an AI-assisted command console for game masters, lore keepers, and world builders."
        }
      ]
    },
    {
      kind: "paragraph",
      inlines: [
        { kind: "text", text: "Type " },
        { kind: "command_ref", label: "help", command: "help" },
        { kind: "text", text: " to see available commands." }
      ]
    }
  ]
};

type BootTaskInfo = { id: string; label: string };
type BootPlan = { needs_setup: boolean; tasks: BootTaskInfo[] };
type BootTaskResult = { ok: boolean; tone: string; detail: string };

const delay = (ms: number): Promise<void> => new Promise((resolve) => window.setTimeout(resolve, ms));

const HISTORY_STORAGE_KEY = "dnd-assistant.command-history";
const MAX_COMMAND_HISTORY = 50;

export default function App() {
  const [entries, setEntries] = createSignal<HistoryEntry[]>([
    {
      id: 1,
      kind: "banner",
      text: BANNER_TEXT,
      outputDoc: BANNER_DOC
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
  // The structured wizard view from the backend's last response, so the next
  // `continue`/`reroll` shows the right generation spinner. null = no wizard active.
  const [wizardView, setWizardView] = createSignal<WizardView | null>(null);
  const [suggestions, setSuggestions] = createSignal<SuggestionViewItem[]>([]);
  const [scrollbarCompensationPx, setScrollbarCompensationPx] = createSignal(0);

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
            : outputDoc ?? buildEntryDoc(kind, text)
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
              : outputDoc ?? buildEntryDoc(kind, text)
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
              : outputDoc ?? buildEntryDoc(kind, text)
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

    const spinnerLabel = commandSpinnerLabel(raw, wizardView(), manifest());
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
      const rendered = responseToRenderableModel(response);
      if (response.ok) {
        if (spinnerId !== null) {
          updateEntry(spinnerId, "spinner", `OK ${spinnerLabel}`);
        }
        // Track the active wizard step from the structured signal so the next
        // `continue`/`reroll` shows the right spinner (story vs. dungeon). Clears
        // itself when the response carries no wizard (the flow finalized/cancelled
        // or no wizard is running).
        setWizardView(response.wizard ?? null);
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
        // Only a doc that leads with an Error-toned status is a hard failure (red).
        // Anything else — e.g. the first-time-setup gate, which leads with a heading
        // — is a soft gate rendered neutrally. Keyed off the doc's structure, not the
        // rendered English.
        const leadBlock = rendered.outputDoc?.blocks[0];
        const isHardError = leadBlock?.kind === "status" && leadBlock.tone === "error";
        appendEntry(isHardError ? "error" : "output", errorText, rendered.outputDoc);
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

  // Boot sequence: run each registered boot task in order, showing a spinner
  // that grows the list, then clear the view and render the welcome/MOTD with
  // accurate connection info. Falls back to the first-time setup message when
  // the app is not configured yet.
  const runBootSequence = async () => {
    if (running()) {
      return;
    }

    setRunning(true);
    try {
      const plan = await invoke<BootPlan>("boot_plan");

      if (plan.needs_setup) {
        const response = await invoke<CommandResponse>("run_command", { input: "status" });
        const rendered = responseToRenderableModel(response);
        const text = response.error || rendered.text || "first-time setup required";
        appendEntry("output", text, rendered.outputDoc);
        return;
      }

      // Details of failed tasks to surface after the view is cleared. The `llm`
      // task is excluded because the MOTD already reports its status.
      const failureNotices: string[] = [];

      for (const task of plan.tasks) {
        const spinnerId = appendEntryWithId("spinner", `${SPINNER_FRAMES[0]} ${task.label} ...`);
        let spinnerFrame = 0;
        const spinnerTimer = window.setInterval(() => {
          spinnerFrame = (spinnerFrame + 1) % SPINNER_FRAMES.length;
          updateEntry(spinnerId, "spinner", `${SPINNER_FRAMES[spinnerFrame]} ${task.label} ...`);
        }, 100);

        try {
          const [result] = await Promise.all([
            invoke<BootTaskResult>("run_boot_task", { id: task.id }),
            delay(BOOT_SPINNER_MIN_MS)
          ]);
          window.clearInterval(spinnerTimer);
          updateEntry(spinnerId, "spinner", `${result.ok ? "OK" : "FAILED"} ${task.label}`);
          if (!result.ok && task.id !== "llm" && result.detail) {
            failureNotices.push(result.detail);
          }
        } catch (error) {
          window.clearInterval(spinnerTimer);
          updateEntry(spinnerId, "spinner", `FAILED ${task.label}`);
          if (task.id !== "llm") {
            failureNotices.push(String(error));
          }
        }
      }

      // Clear the boot spinners and show the welcome banner + accurate status,
      // then re-surface any failures that the MOTD doesn't already cover.
      setEntries([]);
      appendEntry("banner", BANNER_TEXT, BANNER_DOC);
      const motd = await invoke<CommandResponse>("boot_motd");
      const rendered = responseToRenderableModel(motd);
      appendEntry("output", rendered.text || "(ok)", rendered.outputDoc);
      for (const notice of failureNotices) {
        appendEntry("error", notice);
      }
    } catch (error) {
      appendEntry("error", `boot failed: ${String(error)}`);
    } finally {
      setRunning(false);
    }
  };

  const applyClientEvent = (event: CommandClientEvent | null | undefined) => {
    if (!event) {
      return;
    }

    switch (event.kind) {
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
      default:
        return;
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
      case "load_item_draft_with_card":
      case "load_event_draft_with_card":
      case "load_god_draft_with_card":
      case "load_dungeon_draft_with_card":
        return event.entity_card;
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
    void runBootSequence();

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

function responseToRenderableModel(response: CommandResponse): { text: string; outputDoc: OutputDoc | null } {
  const text = segmentsToText(response.segments, response.output);
  return {
    text,
    // Backend responses always carry a doc now; the fallback is purely defensive
    // (and non-parsing) for the rare case one doesn't.
    outputDoc: response.output_doc ?? buildEntryDoc(response.ok ? "output" : "error", text)
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

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }

  if (target.isContentEditable) {
    return true;
  }

  return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;
}

// Pick the spinner label for a submission made inside a wizard, from the
// structured `WizardView` (no prompt-text matching). A step that declares an
// `awaiting_llm_label` (the dungeon plan/story screens, the onboarding Ollama
// steps) shows it for any *advancing* submission. Inputs that act locally (a
// screen's `reroll`/`set`, or `back`/`cancel`/`help`) spend no LLM/probe call,
// so they get no spinner. The story screen's `reroll` is the one input-dependent
// case: it re-runs Pass 1 (a new story), with its own label.
function wizardSpinnerLabel(wizard: WizardView, lowered: string): string | null {
  const isReroll =
    lowered === "reroll" ||
    lowered === "redo" ||
    lowered.startsWith("reroll ") ||
    lowered.startsWith("redo ");
  if (wizard.step_id === "story_review" && isReroll) {
    return "generating story";
  }
  // The location wizard's review reroll re-runs generation (a new location).
  if (wizard.step_id === "review" && isReroll) {
    return "generating location";
  }
  if (!wizard.awaiting_llm_label) {
    return null;
  }
  const isLocalAction =
    isReroll ||
    lowered === "back" ||
    lowered === "cancel" ||
    lowered === "help" ||
    lowered.startsWith("set ");
  return isLocalAction ? null : wizard.awaiting_llm_label;
}

function commandSpinnerLabel(
  raw: string,
  wizard: WizardView | null,
  manifest: CommandManifest | null,
): string | null {
  const lowered = raw.trim().toLowerCase();
  // Inside a wizard, every submission is intercepted by the wizard runtime, so
  // only the wizard's own spinner logic applies (no `create`/`reroll` fallthrough).
  if (wizard) {
    return wizardSpinnerLabel(wizard, lowered);
  }
  const tokens = lowered.split(/\s+/).filter(Boolean);
  // Help/usage invocations spend no LLM or server call, so they get no spinner.
  if (tokens.includes("help")) {
    return null;
  }
  // The spinner taxonomy (which commands generate, and their labels) lives in the
  // manifest; the frontend just matches the longest command prefix. No command
  // names or labels are re-encoded here.
  let best: { length: number; label: string } | null = null;
  for (const hint of manifest?.spinner_hints ?? []) {
    const hintTokens = hint.command.split(" ");
    if (hintTokens.length > tokens.length) {
      continue;
    }
    if (hintTokens.every((token, index) => token === tokens[index])) {
      if (!best || hintTokens.length > best.length) {
        best = { length: hintTokens.length, label: hint.label };
      }
    }
  }
  return best?.label ?? null;
}
