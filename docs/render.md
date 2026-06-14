# Rendering Architecture Guide

> **Purpose:** This document explains how output rendering works end-to-end after the 2026 refactoring. It defines how to add new output types, new entity cards, and new rendering safely. Future agents MUST read this before modifying `OutputDoc`, `OutputBlock`, `InlineNode`, or any frontend render path.

---

## 1. Overview

The rendering pipeline has a single rule:

> **Structured output (`output_doc`) is the canonical path. Plain text (`output`) is the compatibility fallback.**

The pipeline flows:

```text
Backend Command Handler
    → builds OutputDoc (or CommandClientEvent)
    → returns CommandResponse { output_doc, client_event, output }
    → Tauri serializes to JSON
    → Frontend receives
    → if output_doc exists: OutputRenderer renders it directly
    → else: markdown.ts parses plain text into OutputDoc (fallback)
    → if client_event exists: App.tsx handles side effects (load drafts, clear terminal, etc.)
```

---

## 2. Canonical Types (Single Source of Truth)

All output types live in **`runebound-models`** and are generated into TypeScript via `build.rs`.

| Rust Type | File | TS Generated | Purpose |
|---|---|---|---|
| `OutputDoc` | `runebound-models/src/output.rs` | `OutputDoc` | Container of `OutputBlock` list |
| `OutputBlock` | `runebound-models/src/output.rs` | `OutputBlock` | Tagged union: heading, paragraph, list, code, status, spinner, entity_card |
| `InlineNode` | `runebound-models/src/output.rs` | `InlineNode` | Tagged union: text, command_ref, emphasis, strong, code |
| `EntityCardRow` | `runebound-models/src/output.rs` | `EntityCardRow` | Label + value pair for entity cards |
| `StatusTone` | `runebound-models/src/output.rs` | `StatusTone` | success, info, warning, error |
| `SpinnerState` | `runebound-models/src/output.rs` | `SpinnerState` | running, success, error |
| `CommandResponse` | `runebound-models/src/events.rs` | `CommandResponse` | ok, output, error, output_doc, client_event, segments |
| `CommandClientEvent` | `runebound-models/src/events.rs` | `CommandClientEvent` | LoadNpcDraftWithCard, LoadLocationDraftWithCard, LoadFactionDraftWithCard, ClearDrafts, ClearTerminal, ExitRequested |
| `OutputSegment` | `runebound-models/src/events.rs` | `OutputSegment` | Legacy compatibility text/error segments |

**`core/src/output.rs` is just a re-export.** Do not add new types there. Add them to `runebound-models/src/output.rs` and regenerate TypeScript.

---

## 3. Backend Output Construction

### The Two-Field Pattern

Every command handler returns a `CommandResponse` built with either:

1. **`output_doc`** — Preferred. Explicit structured output.
2. **`output`** — Plain text fallback. Used for simple messages, errors, or during transition.

```rust
use dnd_core::output::{doc, heading, paragraph_text, status, StatusTone, command_ref, text_node, paragraph_with_inlines, list};

// Preferred: structured output
let output_doc = doc()
    .with_block(heading(2, "System Status"))
    .with_block(status(StatusTone::Success, "runebound.sh is connected and ready to work."))
    .with_block(list(vec![
        vec![text_node(format!("vault: {}", vault.root().display()))],
        vec![text_node(format!("ollama endpoint: {}", config.ollama.base_url))],
    ]));

CommandResponse {
    ok: true,
    output: "System Status...",  // keep meaningful for fallback
    output_doc: Some(output_doc),
    client_event: None,
    ...
}
```

### Command Handler Locations

| Output Type | Where It's Built |
|---|---|
| Core commands (`status`, `config`, `help`, `exit`, `setup`) | `core/src/command.rs` handlers |
| Desktop commands (`create`, `npc`, `location`, `faction`, `load`, `show`, `delete`, `undo`, `save`, `reroll`, `cancel`, `clear`, `history`) | `desktop/src-tauri/src/commands/*.rs` handlers |
| Entity cards (NPC, Location, Faction) | `runebound-models/src/drafts.rs` builders |

### Entity Card Builders

Entity cards are `OutputDoc` builders in `runebound-models/src/drafts.rs`:

```rust
pub fn npc_entity_card(draft: &NpcDraft) -> OutputDoc;
pub fn location_entity_card(draft: &LocationDraft) -> OutputDoc;
pub fn faction_entity_card(draft: &FactionDraft) -> OutputDoc;
```

These are called by desktop command handlers when returning a `CommandClientEvent`:

```rust
// In desktop/src-tauri/src/commands/npc_commands.rs
let entity_card_doc = npc_entity_card(&normalized_draft);
CommandClientEvent::LoadNpcDraftWithCard {
    draft: normalized_draft,
    entity_card: entity_card_doc,
}
```

**Rule:** All new entity types MUST have an `*_entity_card()` builder in `runebound-models/src/drafts.rs`.

---

## 4. Frontend Render Path

### Files

| File | Role |
|---|---|
| `desktop/src/output/renderer.tsx` | **Canonical renderer.** Renders `OutputDoc` → SolidJS JSX. Supports all `OutputBlock` and `InlineNode` variants. |
| `desktop/src/output/markdown.ts` | **Fallback parser.** Converts plain text strings into `OutputDoc` when `output_doc` is absent. |
| `desktop/src/output/theme.ts` | **Theme mapping.** Maps `StatusTone` and `SpinnerState` to CSS class names. |
| `desktop/src/index.css` | **Styling tokens.** `rb-*` CSS classes for all output blocks. |
| `desktop/src/App.tsx` | **Integration.** Receives `CommandResponse`, decides whether to use `OutputRenderer` or `parseOutputEntry`, and handles `CommandClientEvent` side effects. |

### Render Decision Flow

```
App.tsx receives CommandResponse
    → if response.output_doc exists:
        → <OutputRenderer doc={response.output_doc} />
    → else if response.client_event is Load*DraftWithCard:
        → <OutputRenderer doc={event.entity_card} />  (entity card from event)
    → else:
        → parseOutputEntry(kind, response.output, resolveCommandTarget)
        → <OutputRenderer doc={parsed} />
```

### OutputRenderer Capabilities

`OutputRenderer` in `renderer.tsx` handles all `OutputBlock` variants:

- `heading` → `div.rb-heading-line`
- `paragraph` → `div` with inline nodes
- `list` → `div` with `rb-list-item` entries
- `code` → `pre.rb-code-block`
- `status` → `div` with `rb-status rb-status-{tone}`
- `spinner` → `div` with `rb-spinner` + glyph extraction
- `entity_card` → `div.rb-entity-card` with header and rows

And all `InlineNode` variants:

- `text` → `<span>`
- `command_ref` → `<button class="rb-command-ref">` with `onRunCommand`
- `emphasis` → `<em>`
- `strong` → `<strong>`
- `code` → `<code>`

### Markdown Fallback Parser

`parseOutputEntry()` in `markdown.ts` handles:

- `kind === "error"` → `status` block with tone `error`
- `kind === "info"` → `status` block with tone `info`
- `kind === "spinner"` → `spinner` block with state detection (running/success/error)
- `kind === "banner" | "output"` → Full markdown-inspired parsing:
  - `## Heading` → `heading` block
  - `- List item` → `list` block
  - `` `command` `` → `code` inline or `command_ref` if resolvable
  - Plain text → `paragraph` with inline command detection

### Command Clickability (Two Paths)

1. **Best path:** Backend emits `InlineNode::CommandRef { label, command }`. The frontend renders a clickable button that runs the command directly.
2. **Fallback path:** `markdown.ts` detects command-like text via regex and `resolveCommandTarget()`. This is heuristic-based and less reliable.

**Rule:** Always prefer explicit `command_ref` in backend output. Never rely on the parser fallback for new commands.

---

## 5. Adding New Output Blocks

### Adding a New `OutputBlock` Variant

**Example:** Adding a `quote` block.

1. **`runebound-models/src/output.rs`**
   ```rust
   pub enum OutputBlock {
       // ... existing variants
       Quote {
           text: String,
           attribution: Option<String>,
       },
   }
   ```

2. **`desktop/src/output/renderer.tsx`**
   ```tsx
   if (block.kind === "quote") {
     return (
       <blockquote class="rb-quote">
         {block.text}
         {block.attribution && <cite>{block.attribution}</cite>}
       </blockquote>
     );
   }
   ```

3. **`desktop/src/output/theme.ts`** (if needed)
   ```ts
   export const quoteClass = "rb-quote";
   ```

4. **`desktop/src/index.css`** (styling)
   ```css
   .rb-quote { border-left: 3px solid var(--accent); padding-left: 1rem; font-style: italic; }
   ```

5. **`desktop/src/output/markdown.ts`** (fallback parser, if needed)
   ```ts
   // Add parsing for `> Quote text` or similar markdown pattern
   ```

6. **Regenerate TypeScript types**
   ```bash
   cargo build -p runebound-models  # triggers build.rs
   ```

7. **Update backend helpers** (optional)
   ```rust
   // In runebound-models/src/output.rs
   pub fn quote(text: impl Into<String>, attribution: Option<String>) -> OutputBlock {
       OutputBlock::Quote { text: text.into(), attribution }
   }
   ```

### Adding a New `InlineNode` Variant

**Example:** Adding a `link` inline.

1. **`runebound-models/src/output.rs`**
   ```rust
   pub enum InlineNode {
       // ... existing variants
       Link { text: String, url: String },
   }
   ```

2. **`desktop/src/output/renderer.tsx`**
   ```tsx
   if (inline.kind === "link") {
     return <a href={inline.url} target="_blank" rel="noopener">{inline.text}</a>;
   }
   ```

3. **Regenerate TypeScript types**
   ```bash
   cargo build -p runebound-models
   ```

---

## 6. Adding New Entity Cards

When adding a new entity type (e.g., `Quest`), you must provide an entity card builder.

1. **`runebound-models/src/drafts.rs`**
   - Add `QuestDraft` struct
   - Add `quest_entity_card()` function:
   ```rust
   pub fn quest_entity_card(draft: &QuestDraft) -> OutputDoc {
       let rows = vec![
           entity_row("Objective:", normalize_unknown_text(&draft.objective)),
           entity_row("Giver:", normalize_unknown_text(&draft.giver)),
           // ... more rows
       ];
       doc()
           .with_block(entity_card(&draft.name, rows))
           .with_block(paragraph_with_inlines(vec![
               text_node("Use "),
               command_ref("save", "save"),
               text_node(" to persist this quest."),
           ]))
   }
   ```

2. **`desktop/src-tauri/src/commands/quest_commands.rs`**
   - Build the event with the entity card:
   ```rust
   let entity_card_doc = quest_entity_card(&draft);
   CommandClientEvent::LoadQuestDraftWithCard {
       draft,
       entity_card: entity_card_doc,
   }
   ```

3. **`desktop/src/App.tsx`**
   - Add `questDraft` signal
   - Add rendering branch for `CommandClientEvent::LoadQuestDraftWithCard`
   - Render `<OutputRenderer doc={event.entity_card} />`

4. **`desktop/src/output/renderer.tsx`**
   - `entity_card` is generic. No changes needed if you use the standard `EntityCard` block.

---

## 7. Golden Rules

1. **Prefer `output_doc` over plain `output`.** Always build structured output when possible.
2. **Use `command_ref` for actionable commands.** Never rely on the markdown parser to detect clickable commands.
3. **Add new types to `runebound-models` first.** Then regenerate TypeScript. Never duplicate types.
4. **Keep entity cards in `runebound-models/src/drafts.rs`.** They are model-specific output builders.
5. **Don't touch `core/src/output.rs` directly.** It's a re-export. Edit `runebound-models/src/output.rs`.
6. **Don't add regex hacks to `App.tsx`.** The parser lives in `markdown.ts`. The renderer lives in `renderer.tsx`.
7. **Don't add ad-hoc inline styles.** Use `theme.ts` for class mapping and `index.css` for actual styling.

---

## 8. Anti-Patterns

| Anti-Pattern | Why It's Wrong | Correct Approach |
|---|---|---|
| Building `OutputDoc` with raw JSON instead of helper functions | Fragile, misses type safety | Use `doc()`, `heading()`, `status()`, `paragraph_with_inlines()`, `entity_card()`, etc. |
| Returning plain text for complex output (status, lists, cards) | Loses clickability, theming, and accessibility | Build an `OutputDoc` |
| Adding new `OutputBlock` variants only in the frontend | Backend can't emit them | Add to `runebound-models/src/output.rs` first |
| Detecting clickable commands in `App.tsx` with regex | `App.tsx` is integration logic, not parsing | Use `markdown.ts` for fallback detection; use `command_ref` for explicit paths |
| Duplicating `EntityCardRow` rendering in `App.tsx` | `OutputRenderer` already handles `entity_card` | Emit `OutputDoc` with `entity_card` block and let `OutputRenderer` handle it |
| Hardcoding CSS class names in `renderer.tsx` | Breaks theme abstraction | Import from `theme.ts` |
| Forgetting to regenerate TypeScript after changing `runebound-models` | Frontend will have stale types | Run `cargo build -p runebound-models` |

---

## 9. Current Known Constraints

- Some legacy paths (onboarding, certain error messages) still emit plain text that relies on `markdown.ts` parsing.
- The goal state is **100% explicit `OutputDoc` emission** for all command paths.
- `markdown.ts` should eventually become a compatibility-only fallback, not the primary path.
- `Spinner` blocks are rendered specially: the glyph is extracted from the text prefix, and the state is inferred from text content if not explicitly set.

---

## 10. Suggested Next Increments

1. **Expand `output_doc` coverage for desktop commands**
   - `create_commands.rs`, `entity_commands.rs`, `system_commands.rs` should emit structured output instead of plain strings.
   - Example: `delete` output should be an `OutputDoc` with `status` and `list` blocks, not a hand-formatted string.

2. **Emit `OutputDoc` directly from `CommandClientEvent` responses**
   - Some desktop handlers return `ok_response(text, None)` when they could return `ok_response_with_doc(text, Some(doc), None)`.

3. **Reduce `markdown.ts` to compatibility-only**
   - Once all commands emit `output_doc`, `parseOutputEntry` becomes a thin fallback for edge cases.

4. **Add `output_doc` to error responses**
   - Error responses currently set `output_doc: Some(output_doc_from_error_text(...))`. Make error docs richer with explicit `command_ref` suggestions.

---

## 11. Verification Checklist

Before merging any rendering change:

- [ ] `cargo build -p runebound-models` passes and TypeScript types are regenerated.
- [ ] `make build` (or equivalent) passes.
- [ ] New `OutputBlock` / `InlineNode` variant is handled in `renderer.tsx`.
- [ ] New `OutputBlock` / `InlineNode` has CSS classes in `index.css` (if visible).
- [ ] If plain text fallback matters, `markdown.ts` parses it correctly.
- [ ] Startup banner/help commands render correctly.
- [ ] `status`, `config show`, `config test` render with stable structure.
- [ ] Entity cards (NPC, Location, Faction) render correctly.
- [ ] Clickable commands (`command_ref`) execute the correct command.
- [ ] No new ad-hoc output regex or inline styles added in `App.tsx`.
- [ ] Keyboard UX invariants still work (`Enter`, `Tab`, arrows, `Ctrl+C`).

---

## 12. Quick Reference: Where to Add Code

| Task | File |
|---|---|
| New output type (block or inline) | `runebound-models/src/output.rs` → regenerate TS → `renderer.tsx` |
| New entity card builder | `runebound-models/src/drafts.rs` |
| New rendering logic | `desktop/src/output/renderer.tsx` |
| New theme class mapping | `desktop/src/output/theme.ts` |
| New CSS styles | `desktop/src/index.css` |
| Plain text fallback parsing | `desktop/src/output/markdown.ts` |
| Backend output building | `core/src/command.rs` (core) or `commands/*.rs` (desktop) |
| Frontend integration | `desktop/src/App.tsx` |

---

*Last updated: 2026-06-14*  
*If this document is outdated, update it before adding new output types.*
