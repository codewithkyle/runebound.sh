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
    inputRef?.focus();
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
                onInput={(event) => setCommand(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "c") {
                    if (command()) {
                      event.preventDefault();
                      clearCommand();
                    }
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

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }

  if (target.isContentEditable) {
    return true;
  }

  return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;
}
