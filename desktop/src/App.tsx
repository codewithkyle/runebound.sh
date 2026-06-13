import { invoke } from "@tauri-apps/api/core";
import { For, Show, createEffect, createMemo, createSignal, onMount } from "solid-js";
import { buildSuggestions as buildAutocompleteSuggestions, type SuggestionItem } from "./command/autocomplete";
import { loadManifest, parseInput, type CommandManifest, type ParseResult } from "./command/parser-client";
import { parseOutputEntry } from "./output/markdown";
import { OutputRenderer } from "./output/renderer";
import type { OutputDoc } from "./output/types";
import {
  getSetupState,
  probeOllama,
  saveOnboardingConfig,
  validateVaultPath,
  type SetupScope
} from "./onboarding/client";

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
  const [onboardingActive, setOnboardingActive] = createSignal(false);
  const [onboardingStep, setOnboardingStep] = createSignal(0);
  const [vaultPath, setVaultPath] = createSignal("");
  const [ollamaBaseUrl, setOllamaBaseUrl] = createSignal("http://127.0.0.1:11434");
  const [ollamaModels, setOllamaModels] = createSignal<string[]>([]);
  const [selectedModel, setSelectedModel] = createSignal("");
  const [setupScope, setSetupScope] = createSignal<SetupScope>("workspace");

  const commandMeta = createMemo(() => buildCommandMeta(manifest()));

  const suggestionList = createMemo(() => {
    if (command().trim().length === 0 || running() || suggestionsDismissed()) {
      return [] as SuggestionItem[];
    }

    return buildAutocompleteSuggestions(command(), manifest(), parsedInput());
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

  const normalizeOllamaInput = (value: string): string => {
    const trimmed = value.trim();
    if (!trimmed) {
      return "";
    }
    if (trimmed.includes("://")) {
      return trimmed;
    }
    return `http://${trimmed}`;
  };

  const probeOllamaWithSpinner = async (baseUrl: string) => {
    const normalized = normalizeOllamaInput(baseUrl);
    const spinnerId = appendEntryWithId("spinner", `${SPINNER_FRAMES[0]} checking Ollama at ${normalized} ...`);
    let frame = 0;
    const timer = window.setInterval(() => {
      frame = (frame + 1) % SPINNER_FRAMES.length;
      updateEntry(spinnerId, "spinner", `${SPINNER_FRAMES[frame]} checking Ollama at ${normalized} ...`);
    }, 100);

    try {
      const result = await probeOllama(normalized, 15);
      if (result.ok) {
        updateEntry(spinnerId, "spinner", `OK connected to Ollama at ${normalized}`);
      } else {
        updateEntry(spinnerId, "spinner", `FAILED to connect to Ollama at ${normalized}`);
      }
      return { normalized, result };
    } catch (error) {
      updateEntry(spinnerId, "spinner", `FAILED to connect to Ollama at ${normalized}`);
      throw error;
    } finally {
      window.clearInterval(timer);
    }
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

    const onboarding = await runOnboardingCommand(raw);
    if (onboarding.handled) {
      if (onboarding.recordHistory) {
        pushCommandHistory(raw);
      }
      return;
    }

    setRunning(true);
    try {
      const response = await invoke<CommandResponse>("run_command", { input: raw });
      const rendered = responseToRenderableModel(response, commandMeta());
      if (response.ok) {
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
        appendEntry("output", rendered.text || "(ok)", rendered.outputDoc);
        return;
      }

      const errorText = response.error || rendered.text || "command failed";
      if (isBootstrapSetupMessage(errorText)) {
        appendEntry("output", errorText, rendered.outputDoc);
        appendEntry(
          "info",
          "bootstrap tip: run config init --vault-path <path> --ollama-base-url <url> --model <name> then run status again."
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

  const appendOnboardingIntro = () => {
    appendEntry(
      "output",
      [
        "## First-Time Setup",
        "runebound.sh integrates with your Obsidian vault and a local Ollama model.",
        "Type start setup to begin guided onboarding.",
        "Type setup help to see available setup commands."
      ].join("\n")
    );
  };

  const appendOnboardingHelp = () => {
    appendEntry(
      "output",
      [
        "## Setup commands",
        "start setup",
        "set vault <path>",
        "set ollama <url>",
        "test ollama",
        "set model <name>",
        "use model <index>",
        "scope workspace|global|auto",
        "show setup",
        "save setup",
        "cancel setup"
      ].join("\n")
    );
  };

  const appendOnboardingSummary = () => {
    appendEntry(
      "output",
      [
        "## Current setup",
        `vault: ${vaultPath() || "(not set)"}`,
        `ollama: ${ollamaBaseUrl() || "(not set)"}`,
        `model: ${selectedModel() || "(not set)"}`,
        `scope: ${setupScope()}`
      ].join("\n")
    );
  };

  const startOnboardingFlow = () => {
    setOnboardingStep(1);
    appendEntry(
      "output",
      [
        "## Step 1: Vault Path",
        "runebound.sh needs your Obsidian vault directory so it can read and write your campaign content.",
        "Enter your vault directory path and press Enter.",
        "Example: /path/to/your/Obsidian/Vault"
      ].join("\n")
    );
  };

  const runOnboardingCommand = async (raw: string): Promise<{ handled: boolean; ok: boolean; recordHistory: boolean }> => {
    const trimmed = raw.trim();
    const lowered = trimmed.toLowerCase();

    if (lowered === "start setup") {
      if (!onboardingActive()) {
        setOnboardingActive(true);
      }
      if (onboardingStep() === 0) {
        startOnboardingFlow();
      } else {
        appendEntry("info", "setup already started. use show setup or continue with next step.");
      }
      return { handled: true, ok: true, recordHistory: true };
    }

    if (lowered === "setup help") {
      if (!onboardingActive()) {
        setOnboardingActive(true);
        setOnboardingStep(0);
        appendOnboardingIntro();
      }
      appendOnboardingHelp();
      return { handled: true, ok: true, recordHistory: true };
    }

    if (!onboardingActive()) {
      return { handled: false, ok: false, recordHistory: false };
    }

    if (lowered === "show setup") {
      appendOnboardingSummary();
      return { handled: true, ok: true, recordHistory: true };
    }

    if (lowered === "cancel setup") {
      setOnboardingActive(false);
      setOnboardingStep(0);
      appendEntry("info", "setup cancelled. run start setup anytime to continue.");
      return { handled: true, ok: true, recordHistory: true };
    }

    const vaultMatch = trimmed.match(/^set\s+vault\s+(.+)$/i);
    if (vaultMatch) {
      const value = vaultMatch[1].trim();
      try {
        await validateVaultPath(value);
        setVaultPath(value);
        if (onboardingStep() < 2) {
          setOnboardingStep(2);
        }
        appendEntry(
          "output",
          [
            "## Step 2: Ollama server",
            `vault set to: ${value}`,
            "Enter your Ollama URL and press Enter.",
            "Example: http://127.0.0.1:11434"
          ].join("\n")
        );
        return { handled: true, ok: true, recordHistory: true };
      } catch (error) {
        appendEntry("info", String(error));
        return { handled: true, ok: false, recordHistory: true };
      }
    }

    const ollamaMatch = trimmed.match(/^set\s+ollama\s+(.+)$/i);
    if (ollamaMatch) {
      const value = normalizeOllamaInput(ollamaMatch[1]);
      setOllamaBaseUrl(value);
      if (onboardingStep() < 2) {
        setOnboardingStep(2);
      }
      appendEntry("output", `ollama URL set to: ${value}\nrun test ollama to verify connection.`);
      return { handled: true, ok: true, recordHistory: true };
    }

    if (lowered === "test ollama") {
      try {
        const { normalized, result } = await probeOllamaWithSpinner(ollamaBaseUrl().trim());
        setOllamaBaseUrl(normalized);
        if (!result.ok) {
          appendEntry("info", result.detail);
          return { handled: true, ok: false, recordHistory: true };
        }

        setOllamaModels(result.models);
        if (!selectedModel()) {
          if (result.models.length > 0) {
            setSelectedModel(result.models[0]);
          }
        }
        setOnboardingStep(3);

        const modelLines = result.models.length
          ? result.models.map((model, index) => `${index + 1}: ${model}`)
          : ["(no models returned)"];
        appendEntry(
          "output",
          [
            "## Step 3: Model",
            result.detail,
            "Enter a model name and press Enter.",
            "Or enter a model number from the list below.",
            ...modelLines
          ].join("\n")
        );
        return { handled: true, ok: true, recordHistory: true };
      } catch (error) {
        appendEntry("info", String(error));
        return { handled: true, ok: false, recordHistory: true };
      }
    }

    const useModelMatch = trimmed.match(/^use\s+model\s+(\d+)$/i);
    if (useModelMatch) {
      const index = Number.parseInt(useModelMatch[1], 10);
      const models = ollamaModels();
      if (Number.isNaN(index) || index < 1 || index > models.length) {
        appendEntry("info", `model index out of range: ${useModelMatch[1]}`);
        return { handled: true, ok: false, recordHistory: true };
      }
      setSelectedModel(models[index - 1]);
      if (onboardingStep() < 4) {
        setOnboardingStep(4);
      }
      appendEntry(
        "output",
        [
          `model selected: ${models[index - 1]}`,
          "## Step 4: Save config",
          "Enter scope: workspace, global, or auto."
        ].join("\n")
      );
      return { handled: true, ok: true, recordHistory: true };
    }

    const setModelMatch = trimmed.match(/^set\s+model\s+(.+)$/i);
    if (setModelMatch) {
      const value = setModelMatch[1].trim();
      if (!value) {
        appendEntry("info", "model name cannot be empty");
        return { handled: true, ok: false, recordHistory: true };
      }
      setSelectedModel(value);
      if (onboardingStep() < 4) {
        setOnboardingStep(4);
      }
      appendEntry(
        "output",
        [
          `model set to: ${value}`,
          "## Step 4: Save config",
          "Enter scope: workspace, global, or auto."
        ].join("\n")
      );
      return { handled: true, ok: true, recordHistory: true };
    }

    const scopeMatch = trimmed.match(/^scope\s+(workspace|global|auto)$/i);
    if (scopeMatch) {
      const value = scopeMatch[1].toLowerCase() as SetupScope;
      setSetupScope(value);
      appendEntry("output", `scope set to: ${value}\nType save setup when ready.`);
      return { handled: true, ok: true, recordHistory: true };
    }

    if (onboardingStep() === 1) {
      if (!trimmed) {
        appendEntry("info", "Enter a vault directory path to continue setup.");
        return { handled: true, ok: false, recordHistory: false };
      }

      try {
        await validateVaultPath(trimmed);
        setVaultPath(trimmed);
        if (onboardingStep() < 2) {
          setOnboardingStep(2);
        }
        appendEntry(
          "output",
          [
            "## Step 2: Ollama server",
            `vault set to: ${trimmed}`,
            "Enter your Ollama URL and press Enter.",
            "Example: http://127.0.0.1:11434"
          ].join("\n")
        );
        return { handled: true, ok: true, recordHistory: true };
      } catch (error) {
        appendEntry("info", String(error));
        return { handled: true, ok: false, recordHistory: true };
      }
    }

    if (onboardingStep() === 2) {
      if (!trimmed) {
        appendEntry("info", "Enter your Ollama URL to continue setup.");
        return { handled: true, ok: false, recordHistory: false };
      }

      const normalized = normalizeOllamaInput(trimmed);
      setOllamaBaseUrl(normalized);
      try {
        const { result } = await probeOllamaWithSpinner(normalized);
        if (!result.ok) {
          appendEntry("info", result.detail);
          return { handled: true, ok: false, recordHistory: true };
        }

        setOllamaModels(result.models);
        if (!selectedModel() && result.models.length > 0) {
          setSelectedModel(result.models[0]);
        }
        setOnboardingStep(3);

        const modelLines = result.models.length
          ? result.models.map((model, index) => `${index + 1}: ${model}`)
          : ["(no models returned)"];

        appendEntry(
          "output",
          [
            "## Step 3: Model",
            result.detail,
            "Enter a model name and press Enter.",
            "Or enter a model number from the list below.",
            ...modelLines
          ].join("\n")
        );
        return { handled: true, ok: true, recordHistory: true };
      } catch (error) {
        appendEntry("info", String(error));
        return { handled: true, ok: false, recordHistory: true };
      }
    }

    if (onboardingStep() === 3) {
      if (!trimmed) {
        appendEntry("info", "Enter a model name or number to continue setup.");
        return { handled: true, ok: false, recordHistory: false };
      }

      const index = Number.parseInt(trimmed, 10);
      if (!Number.isNaN(index) && index >= 1 && index <= ollamaModels().length) {
        const picked = ollamaModels()[index - 1];
        setSelectedModel(picked);
        setOnboardingStep(4);
        appendEntry(
          "output",
          [
            `model selected: ${picked}`,
            "## Step 4: Save config",
            "Enter scope: workspace, global, or auto."
          ].join("\n")
        );
        return { handled: true, ok: true, recordHistory: true };
      }

      setSelectedModel(trimmed);
      setOnboardingStep(4);
      appendEntry(
        "output",
        [
          `model set to: ${trimmed}`,
          "## Step 4: Save config",
          "Enter scope: workspace, global, or auto."
        ].join("\n")
      );
      return { handled: true, ok: true, recordHistory: true };
    }

    if (onboardingStep() === 4) {
      const rawScope = trimmed.toLowerCase();
      if (rawScope === "workspace" || rawScope === "global" || rawScope === "auto") {
        setSetupScope(rawScope as SetupScope);
        appendEntry("output", `scope set to: ${rawScope}\nType save setup to finish.`);
        return { handled: true, ok: true, recordHistory: true };
      }
      if (rawScope === "save") {
        const saveResult = await runOnboardingCommand("save setup");
        return { handled: true, ok: saveResult.ok, recordHistory: true };
      }
    }

    if (lowered === "save setup") {
      if (!vaultPath().trim()) {
        appendEntry("info", "vault path is missing. run set vault <path>." );
        return { handled: true, ok: false, recordHistory: true };
      }
      if (!ollamaBaseUrl().trim()) {
        appendEntry("info", "ollama URL is missing. run set ollama <url>." );
        return { handled: true, ok: false, recordHistory: true };
      }
      if (!selectedModel().trim()) {
        appendEntry("info", "model is missing. run set model <name> or use model <index>." );
        return { handled: true, ok: false, recordHistory: true };
      }

      try {
        const result = await saveOnboardingConfig({
          vault_path: vaultPath().trim(),
          ollama_base_url: ollamaBaseUrl().trim(),
          model: selectedModel().trim(),
          scope: setupScope()
        });

        appendEntry(
          "output",
          [
            "## Onboarding complete",
            `config saved: ${result.config_path}`,
            `vault ready: ${result.vault_path}`,
            `database ready: ${result.db_path}`
          ].join("\n")
        );
        if (result.warnings.length > 0) {
          appendEntry("info", `setup warnings:\n- ${result.warnings.join("\n- ")}`);
        }

        setOnboardingActive(false);
        setOnboardingStep(0);
        void runStartupStatusCheck();
        return { handled: true, ok: true, recordHistory: true };
      } catch (error) {
        appendEntry("info", String(error));
        return { handled: true, ok: false, recordHistory: true };
      }
    }

    appendEntry("info", "setup mode is active. use setup help to continue guided onboarding.");
    return { handled: true, ok: false, recordHistory: true };
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
    void getSetupState()
      .then((setup) => {
        setOllamaBaseUrl(setup.default_ollama_base_url || "http://127.0.0.1:11434");
        if (setup.needs_setup) {
          setOnboardingActive(true);
          setOnboardingStep(0);
          appendOnboardingIntro();
          if (setup.issues.length > 0) {
            appendEntry("info", `missing: ${setup.issues.join("; ")}`);
          }
          return;
        }

        void runStartupStatusCheck();
      })
      .catch(() => {
        void runStartupStatusCheck();
      });

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

  if (trimmed === "!!") {
    return true;
  }
  if (/^!\d+$/.test(trimmed)) {
    return true;
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
