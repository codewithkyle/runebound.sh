import { invoke } from "@tauri-apps/api/core";
import { For, createSignal, onMount } from "solid-js";

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

const COMMAND_HINTS = ["status", "config show", "config test", "config doctor", "config init --vault-path <path> --ollama-base-url <url> --model <name>"];

export default function App() {
  const [entries, setEntries] = createSignal<HistoryEntry[]>([
    {
      id: 1,
      kind: "info",
      text: "DND Assistant desktop shell ready. Try `status` or `config show`."
    }
  ]);
  const [command, setCommand] = createSignal("");
  const [running, setRunning] = createSignal(false);

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
        outputRef.scrollTop = outputRef.scrollHeight;
      }
    });
  };

  const submitCommand = async () => {
    const raw = command().trim();
    if (!raw || running()) {
      return;
    }

    appendEntry("input", `> ${raw}`);
    setCommand("");
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
  });

  return (
    <div class="h-screen bg-bg text-text font-mono flex flex-col">
      <header class="border-b border-border px-4 py-3 bg-surface flex items-center justify-between">
        <h1 class="text-accent text-sm tracking-wide uppercase">DND Assistant</h1>
        <span class="text-muted text-xs">Gruvbox Dark</span>
      </header>

      <main ref={outputRef} class="flex-1 overflow-y-auto px-4 py-4 bg-bg">
        <div class="space-y-3">
          <For each={entries()}>
            {(entry) => (
              <pre class={entryClass(entry.kind)}>{entry.text}</pre>
            )}
          </For>
        </div>
      </main>

      <section class="border-t border-border bg-surface px-4 py-3">
        <div class="mb-2 flex flex-wrap gap-2">
          <For each={COMMAND_HINTS}>
            {(hint) => <span class="text-xs px-2 py-1 rounded bg-surface2 text-muted border border-border">{hint}</span>}
          </For>
        </div>

        <form
          class="flex items-center gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void submitCommand();
          }}
        >
          <span class="text-accent">$</span>
          <input
            ref={inputRef}
            class="flex-1 bg-surface2 border border-border rounded px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-accent"
            type="text"
            placeholder="Type a command..."
            value={command()}
            onInput={(event) => setCommand(event.currentTarget.value)}
          />
          <button
            type="submit"
            disabled={running()}
            class="px-3 py-2 rounded border border-border text-xs uppercase tracking-wide text-text bg-surface2 hover:bg-surface disabled:opacity-50"
          >
            {running() ? "Running" : "Run"}
          </button>
        </form>
      </section>
    </div>
  );
}

function entryClass(kind: EntryKind): string {
  const base = "whitespace-pre-wrap break-words text-sm leading-6 rounded border px-3 py-2";
  if (kind === "input") {
    return `${base} border-accent text-accent bg-surface`;
  }
  if (kind === "error") {
    return `${base} border-error text-error bg-surface`;
  }
  if (kind === "info") {
    return `${base} border-info text-info bg-surface`;
  }
  return `${base} border-border text-text bg-surface`;
}
