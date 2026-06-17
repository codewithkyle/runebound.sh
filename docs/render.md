# Rendering Architecture Guide

> **Purpose:** This document explains output rendering end-to-end and provides implementation guidance for new output blocks and custom entity cards.

---

## 1. Overview

Primary rule:

> **`output_doc` is canonical. `output` is fallback.**

Pipeline:

```text
Command handler
  -> build OutputDoc and/or CommandClientEvent
  -> return CommandResponse { output_doc, client_event, output }
  -> frontend receives response
  -> render output_doc if present
  -> else parse output text via markdown fallback
  -> apply client_event side effects (draft state, clear terminal, exit)
```

---

## 2. Canonical Types

All render contracts live in `runebound-models` and are generated to TS.

| Rust Type | File | Purpose |
|---|---|---|
| `OutputDoc` | `runebound-models/src/output.rs` | root output block collection |
| `OutputBlock` | `runebound-models/src/output.rs` | heading/paragraph/list/code/status/spinner/entity_card |
| `InlineNode` | `runebound-models/src/output.rs` | text/command_ref/emphasis/strong/code |
| `CommandResponse` | `runebound-models/src/events.rs` | backend response payload |
| `CommandClientEvent` | `runebound-models/src/events.rs` | side-effect events including draft-card events |

Do not add output contracts in `core/src/output.rs` (re-export only).

---

## 3. Backend Output Construction

### Preferred Pattern

Return a meaningful `output_doc` plus fallback `output` text.

```rust
use dnd_core::output::{doc, heading, paragraph_with_inlines, status, StatusTone};

let output_doc = doc()
    .with_block(heading(2, "System Status"))
    .with_block(status(StatusTone::Success, "ready"));
```

### Current Output Sources

- Core command handlers: `core/src/command.rs`
- Desktop command handlers: `desktop/src-tauri/src/commands/*.rs`
- Entity card builders: `runebound-models/src/drafts.rs`

### CommandRef Rule

For actionable command guidance, emit explicit `command_ref` inline nodes. Do not rely on markdown parser heuristics for new feature work.

### Wizard Prompt Builders

Wizard step prompts must be built with the sanctioned helpers in the `wizard` crate's `prompt.rs` — `wizard_menu` (numbered/option menu), `action_row` (a `·`-joined review verb row), and `choice_lines` (one choice per line). Each renders a `WizardChoice` as a `command_ref`, so clickability is guaranteed by construction. Never hand-build a wizard prompt from `paragraph_text` with back-tick verbs — that produces non-clickable code spans (the dungeon-flow regression).

---

## 4. Frontend Render Path

| File | Role |
|---|---|
| `desktop/src/output/renderer.tsx` | canonical renderer from `OutputDoc` to JSX |
| `desktop/src/output/markdown.ts` | compatibility parser for plain-text fallback |
| `desktop/src/output/theme.ts` | status/spinner class mapping |
| `desktop/src/index.css` | `rb-*` output styles |
| `desktop/src/App.tsx` | integration and client-event handling |

Render decision in `App.tsx`:

1. Use `response.output_doc` if present
2. Else use client-event card doc for draft-load events
3. Else parse plain text with `parseOutputEntry`

---

## 5. Adding New Output Blocks

When adding a new `OutputBlock` or `InlineNode` variant:

1. Add variant in `runebound-models/src/output.rs`
2. Update renderer behavior in `desktop/src/output/renderer.tsx`
3. Update theme mapping in `desktop/src/output/theme.ts` if needed
4. Add CSS in `desktop/src/index.css`
5. Update fallback parser in `desktop/src/output/markdown.ts` only if plain-text compatibility is required
6. Regenerate TS types with `cargo build -p runebound-models`

---

## 6. Adding New Custom Entity Cards

For a new entity type (example: `quest`):

1. Add draft model in `runebound-models/src/drafts.rs`
2. Add `quest_entity_card(draft: &QuestDraft) -> OutputDoc` in `runebound-models/src/drafts.rs`
3. Emit card via client event from desktop command handlers
4. Extend `CommandClientEvent` variants if needed
5. Handle event branch in `desktop/src/App.tsx`
6. Reuse generic `entity_card` renderer unless a genuinely new block type is required

Existing cards live next to their drafts: `npc_entity_card`, `location_entity_card`, `faction_entity_card`, `item_entity_card`, `god_entity_card`, `event_entity_card`, and `dungeon_entity_card`.

Recommended card composition:

- title = entity name
- rows = stable label/value pairs
- footer paragraph with explicit `command_ref` actions (`save`, `reroll`, etc.)

---

## 7. Current Constraints

- Some command paths still return plain string output and depend on markdown fallback.
- Markdown parsing should remain compatibility logic, not the primary authoring path.
- Spinner fallback infers state from text for compatibility, but explicit spinner blocks are preferred.

---

## 8. Anti-Patterns

| Anti-Pattern | Why It Is Wrong | Correct Approach |
|---|---|---|
| Hand-building JSON output docs in handlers | brittle and untyped | use output helper constructors |
| Returning plain text for complex UI output | loses structure and clickability | emit `output_doc` |
| Adding variants only in frontend | backend cannot produce them | update `runebound-models` first |
| Clickable command regexes in `App.tsx` | wrong layer and fragile | emit `command_ref`, keep parser in `markdown.ts` |
| Duplicating card rendering in `App.tsx` | renderer already supports entity cards | emit `entity_card` block and let renderer handle it |

---

## 9. Rendering Checklist

Before merging rendering changes:

- [ ] `runebound-models` types updated first
- [ ] TS types regenerated (`cargo build -p runebound-models`)
- [ ] `renderer.tsx` handles new variants
- [ ] styles added in `index.css` and mappings in `theme.ts` if needed
- [ ] fallback parser updated only when required
- [ ] clickable command refs execute expected commands
- [ ] entity cards render correctly in desktop UI
- [ ] `make build` passes

---

## 10. Quick Reference

| Task | File |
|---|---|
| Add output block / inline type | `runebound-models/src/output.rs` |
| Add entity card builder | `runebound-models/src/drafts.rs` |
| Build a wizard step prompt | `wizard` crate `prompt.rs` (`wizard_menu`/`action_row`) |
| Add rendering behavior | `desktop/src/output/renderer.tsx` |
| Add fallback parse behavior | `desktop/src/output/markdown.ts` |
| Add output styling | `desktop/src/index.css` and `desktop/src/output/theme.ts` |
| Integrate events in app shell | `desktop/src/App.tsx` |

---

*Last updated: 2026-06-17*  
*Keep this document aligned with rendering contract changes in the same PR.*
