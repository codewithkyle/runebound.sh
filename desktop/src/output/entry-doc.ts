import type { OutputDoc, SpinnerState } from "../generated/models";

type EntryKind = "input" | "output" | "error" | "info" | "banner" | "spinner";

// Build the OutputDoc for a terminal entry the *frontend itself* generated — a
// spinner frame, an error/info line, or a defensive fallback for the rare case a
// backend response arrives without a doc. It only wraps the given text in a typed
// block; it never inspects prose to guess document structure or which words are
// runnable commands. Clickability comes exclusively from backend-authored
// command_ref nodes (see docs/architecture.md §9).
export function buildEntryDoc(kind: EntryKind, text: string): OutputDoc {
  if (kind === "error") {
    return { blocks: [{ kind: "status", tone: "error", text }] };
  }
  if (kind === "info") {
    return { blocks: [{ kind: "status", tone: "info", text }] };
  }
  if (kind === "spinner") {
    // The state is read from the frontend's own spinner text ("OK ..." /
    // "FAILED ..."), which this code wrote — not from any backend prose.
    const normalized = text.trim();
    const lowered = normalized.replace(/^[⠀-⣿]\s*/, "").toLowerCase();
    const state: SpinnerState = lowered.startsWith("ok")
      ? "success"
      : lowered.startsWith("failed")
        ? "error"
        : "running";
    return { blocks: [{ kind: "spinner", state, text: normalized }] };
  }
  // output / banner / input fallback: a single plain paragraph, never parsed.
  return { blocks: [{ kind: "paragraph", inlines: [{ kind: "text", text }] }] };
}
