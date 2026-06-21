# Rendering Architecture Guide

> **Purpose:** This document explains output rendering end-to-end and provides implementation guidance for new output blocks, custom entity cards, and reference-library card content.

---

## 1. Overview

Primary rule:

> **`output_doc` is canonical. The frontend never parses prose.**

A command handler returns a `CommandResponse`. The backend authors a structured
`OutputDoc` (and/or a `CommandClientEvent` carrying a card); the frontend renders
that doc directly. There is **no** markdown/heuristic parser on the frontend —
clickability comes exclusively from backend-authored `command_ref` nodes. The only
"fallback" is `buildEntryDoc`, which wraps a bare string in a single typed block
(it does not inspect the text).

Pipeline:

```text
Command handler
  -> build OutputDoc (and/or CommandClientEvent)
  -> return CommandResponse { ok, output, error, exit_code,
                              segments, output_doc, client_event, wizard }
  -> frontend receives response (App.tsx)
  -> render decision:
       1. a draft-load client_event supplies its own entity_card OutputDoc, OR
       2. response.output_doc if present, OR
       3. buildEntryDoc(kind, segmentsToText(segments, output))  // wrap-only, no parsing
  -> apply client_event side effects (draft state, clear terminal, exit)
```

---

## 2. Canonical Types

All render contracts live in `runebound-models` and are generated to
`desktop/src/generated/models.ts`. Regenerate with
`UPDATE_MODELS=1 cargo test -p runebound-models` (the test is both the generator
and the drift guard — `cargo build` and `make build` do **not** regenerate it).

| Rust Type | File | Purpose |
|---|---|---|
| `OutputDoc` | `runebound-models/src/output.rs` | root block collection (`blocks: Vec<OutputBlock>`) |
| `OutputBlock` | `runebound-models/src/output.rs` | `heading` / `paragraph` / `list` / `code` / `status` / `spinner` / `entity_card` / `image` |
| `InlineNode` | `runebound-models/src/output.rs` | `text` / `command_ref` / `emphasis` / `strong` / `code` |
| `EntityCardRow` | `runebound-models/src/output.rs` | `{ label, value }` rows inside an `entity_card` |
| `StatusTone` / `SpinnerState` | `runebound-models/src/output.rs` | `success`/`info`/`warning`/`error`, `running`/`success`/`error` |
| `CommandResponse` | `runebound-models/src/events.rs` | backend response payload (see below) |
| `CommandClientEvent` | `runebound-models/src/events.rs` | side-effect events incl. `Load<Kind>DraftWithCard { draft, entity_card }`, `ClearDrafts`, `ClearTerminal`, `ExitRequested` |
| `OutputSegment` | `runebound-models/src/events.rs` | `{ kind: text\|error, text, command_ref }` — the legacy segmented text channel |
| `WizardView` | `runebound-models/src/events.rs` | active-wizard signal `{ id, step_id, awaiting_llm_label }` that drives the wizard spinner |

`CommandResponse` carries `ok`, `output`, `error`, `exit_code`, `segments`,
`output_doc`, `client_event`, and `wizard`. `output_doc` is the canonical render
surface; `output`/`segments` are the plain-text channel; `client_event` carries
draft cards and side effects; `wizard` drives the spinner without prompt-text
matching.

Do not add output contracts in `core/src/output.rs` — it is a re-export of
`runebound-models::output` so backends can `use dnd_core::output::*`.

### `OutputBlock` variants

`Heading { level, text }`, `Paragraph { inlines }`, `List { items }`,
`Code { language, text }`, `Status { tone, text }`, `Spinner { state, text }`,
`EntityCard { title, subtitle?, rows, body }`, `Image { src, alt }`.

`EntityCard.body` is a nested `Vec<OutputBlock>` rendered **inside** the card
beneath the label/value rows — this is how a spell/monster card carries a full
description (prose, lists, subsection headings, tables) as one unit. `Image.src`
is a *logical asset key* (not a path); the frontend maps it to a bundled, hashed
URL via `DOC_IMAGES` in `renderer.tsx`, so the backend never knows build paths.

---

## 3. Backend Output Construction

### Preferred Pattern

Build an `OutputDoc` with the helper constructors and return it; `to_plain_text()`
derives the `output` fallback string from the same doc (so the structured and
plain-text forms can't drift, and help/card prose share one source).

```rust
use dnd_core::output::{doc, heading, paragraph_with_inlines, status, StatusTone, command_ref, text_node};

let document = doc()
    .with_block(heading(2, "System Status"))
    .with_block(paragraph_with_inlines(vec![
        text_node("Type "),
        command_ref("status", "status"),
        text_node(" to run checks."),
    ]))
    .with_block(status(StatusTone::Success, "ready"));
```

Constructors (all in `runebound-models/src/output.rs`, re-exported via
`dnd_core::output`): `doc`, `heading`, `paragraph_text`, `paragraph_with_inlines`,
`list`, `code_block`, `status`, `spinner`, `image`, `entity_card`,
`entity_card_full`, `entity_row`, `text_node`, `command_ref`, `emphasis`,
`strong`, `code`, and `render_table` (formats a tiny table into fixed-width text
for a `code_block` — there is no `OutputBlock::Table`).

### Current Output Sources

- Core command handlers: `core/src/command.rs`
- Desktop command handlers: `desktop/src-tauri/src/commands/*.rs`
- Entity card builders: `runebound-models/src/drafts.rs`
- Reference-library card builders: `runebound-models/src/spells.rs` (`spell_card`), `runebound-models/src/monsters.rs` (`monster_card`)

### CommandRef Rule

For actionable command guidance, emit explicit `command_ref` inline nodes. There
is no markdown parser to fall back on — a command that isn't a `command_ref` is
not clickable.

### Wizard Prompt Builders

Wizard step prompts must be built with the sanctioned helpers in the `wizard`
crate's `prompt.rs` — `wizard_menu` (numbered/option menu), `action_row` (a
`·`-joined review verb row), and `choice_lines` (one choice per line). Each
renders a `WizardChoice` as a `command_ref`, so clickability is guaranteed by
construction. Never hand-build a wizard prompt from `paragraph_text` with
back-tick verbs — that produces non-clickable code spans (the dungeon-flow
regression).

---

## 4. Reference-Library Card Rendering & 5etools Markup

Spell and monster cards are the richest content the app renders, and they are
where **cross-links** come from. The mechanism is still "backend authors
`command_ref` nodes" — but the spans are parsed once at import time, not at render
time.

The flow:

```text
import (core/src/{spell,monster}_import.rs)
  -> parse 5etools entry strings: {@spell …}, {@creature …}, {@damage …}, …
  -> core/src/fivetools_markup.rs::render_inline lowers each string to Vec<Span>:
       {@spell Fireball|XPHB}      -> Span::Link { label: "Fireball", command: "spell Fireball" }
       {@creature Goblin|XMM|...}  -> Span::Link { label: "...",      command: "monster Goblin" }
       everything else (incl. {@damage}, {@dc}, wrapper tags) -> Span::Text
  -> store the Spans inside the canonical <slug>.toml card (CardStore<T>)

lookup
  -> load the card from the TOML store
  -> spell_card / monster_card build an entity_card_full, mapping body Spans via
     spans_to_inlines:  Span::Link -> InlineNode::CommandRef,  Span::Text -> text
  -> renderer.tsx renders each CommandRef as a clickable <button> that runs `command`
```

Key pieces:

- **`core/src/fivetools_markup.rs`** is the single shared parser — `strip_tags`
  (plain text), `render_inline` (text + clickable `Span::Link`), and `slugify`
  (the `<slug>` primary key). **Both** importers parse through it, so the two can't
  drift. Only `{@spell}` and `{@creature}` map to commands (`spell`/`monster`);
  every other tag collapses to its display text. Do not add a second markup parser.
- **`Span`** (`runebound-models/src/monsters.rs`): `Text { text }` or
  `Link { label, command }`. `SpellBlock`/`StatBlock` carry `Vec<Span>` for prose
  and bullets; table cells stay plain text.
- **`spans_to_inlines`** (`runebound-models/src/monsters.rs`, shared by both card
  builders) maps `Link → command_ref`, `Text → text_node`. This is the only place
  the lowering happens.
- The card is one `entity_card_full`: title = name, subtitle = the
  "Level 3 Evocation" / "Small Fey, Chaotic Neutral" line, rows = stat lines, body
  = description/sections (with the source as an emphasized footer).

If you add a new tag that should be clickable, extend `tag_link` in
`fivetools_markup.rs` (and add the command target). If you add a new card kind,
follow `docs/feature-development.md` §8 (Playbook G); `docs/spellbook.md` and
`docs/monster-manual.md` are the worked examples.

---

## 5. Frontend Render Path

| File | Role |
|---|---|
| `desktop/src/output/renderer.tsx` | canonical renderer from `OutputDoc` to JSX (`OutputRenderer`) |
| `desktop/src/output/entry-doc.ts` | `buildEntryDoc(kind, text)` — wrap-only fallback (never parses prose) |
| `desktop/src/output/theme.ts` | status/spinner/command-ref class mapping |
| `desktop/src/index.css` | `rb-*` output styles |
| `desktop/src/App.tsx` | integration, render decision, client-event handling, spinner label |

Render decision in `App.tsx` (`responseToRenderableModel` + the call site):

1. If a draft-load `client_event` carries an `entity_card`, render that doc
   (`outputDocFromClientEvent`).
2. Else render `response.output_doc` if present.
3. Else `buildEntryDoc(kind, segmentsToText(response.segments, response.output))`
   — a single typed block wrapping the plain text. **No parsing happens here**;
   `buildEntryDoc` only chooses a block kind (status for error/info, spinner for
   spinner frames, paragraph otherwise).

`renderer.tsx` handles every block kind: `command_ref` inlines render as a
`<button>` whose click calls `onRunCommand(command)`; `image` blocks resolve the
logical `src` key through `DOC_IMAGES`; `entity_card` renders the header
(title + optional subtitle), the label/value rows, and then recurses into `body`.

---

## 6. Adding New Output Blocks

When adding a new `OutputBlock` or `InlineNode` variant:

1. Add the variant in `runebound-models/src/output.rs`
2. Regenerate TS: `UPDATE_MODELS=1 cargo test -p runebound-models`
3. Render the variant in `desktop/src/output/renderer.tsx`
4. Add CSS in `desktop/src/index.css` and a class mapping in `desktop/src/output/theme.ts` if needed
5. There is no fallback parser to update. `buildEntryDoc` only emits a small fixed
   set of blocks for frontend-generated entries; a new backend block type needs no
   change there.

---

## 7. Adding New Custom Entity Cards

For a new entity type (example: `quest`):

1. Add draft model in `runebound-models/src/drafts.rs`
2. Add `quest_entity_card(draft: &QuestDraft) -> OutputDoc` in `runebound-models/src/drafts.rs`
3. Emit the card via a `CommandClientEvent` from desktop command handlers
4. Extend `CommandClientEvent` variants if needed (e.g. `LoadQuestDraftWithCard`)
5. Handle the event branch in `desktop/src/App.tsx`
6. Reuse `entity_card`/`entity_card_full` unless a genuinely new block type is required

Existing draft cards live next to their drafts in `drafts.rs`: `npc_entity_card`,
`location_entity_card`, `faction_entity_card`, `item_entity_card`,
`god_entity_card`, `event_entity_card`, and `dungeon_entity_card`. The
reference-library cards (`spell_card`, `monster_card`) live in `spells.rs` /
`monsters.rs` instead, beside their payload types.

Recommended card composition:

- title = entity name; subtitle = a single classifying line (optional)
- rows = stable label/value pairs (skip empty values)
- body = free-form blocks; footer paragraph with explicit `command_ref` actions (`save`, `reroll`, etc.) for editable drafts

---

## 8. Constraints

- The frontend performs **no** prose parsing; structure and clickability are
  always backend-authored. `buildEntryDoc` is wrap-only.
- `to_plain_text()` is the single source for the `output` fallback string and for
  help/card prose, so structured and plain text can't drift.
- The frontend `commandSpinnerLabel()` infers a spinner label from
  `manifest.spinner_hints` (longest-prefix match); explicit `Spinner` blocks and
  the `WizardView` signal are the structured paths. See `docs/feature-development.md` §9.

---

## 9. Anti-Patterns

| Anti-Pattern | Why It Is Wrong | Correct Approach |
|---|---|---|
| Hand-building JSON output docs in handlers | brittle and untyped | use the output helper constructors |
| Returning plain text for complex UI output | loses structure and clickability | emit `output_doc` |
| Adding variants only in frontend | backend cannot produce them | update `runebound-models` first, then regenerate TS |
| Parsing prose in `App.tsx`/`renderer.tsx` to find commands | wrong layer; there is no parser by design | emit `command_ref` from the backend |
| A second 5etools markup parser | drift between spell/monster rendering | reuse `core/src/fivetools_markup.rs` |
| Duplicating card rendering in `App.tsx` | renderer already supports entity cards | emit an `entity_card` block and let `renderer.tsx` handle it |

---

## 10. Rendering Checklist

Before merging rendering changes:

- [ ] `runebound-models` types updated first
- [ ] TS regenerated (`UPDATE_MODELS=1 cargo test -p runebound-models`)
- [ ] `renderer.tsx` handles new variants
- [ ] styles added in `index.css` and mappings in `theme.ts` if needed
- [ ] clickable command refs execute the expected commands
- [ ] entity/reference cards render correctly in desktop UI
- [ ] `make build` passes

---

## 11. Quick Reference

| Task | File |
|---|---|
| Add output block / inline type | `runebound-models/src/output.rs` |
| Add entity draft card builder | `runebound-models/src/drafts.rs` |
| Add reference-library card builder | `runebound-models/src/spells.rs` / `monsters.rs` |
| Add/extend 5etools markup or a clickable tag | `core/src/fivetools_markup.rs` |
| Build a wizard step prompt | `wizard` crate `prompt.rs` (`wizard_menu`/`action_row`/`choice_lines`) |
| Add rendering behavior | `desktop/src/output/renderer.tsx` |
| Frontend fallback wrapping (not parsing) | `desktop/src/output/entry-doc.ts` |
| Add output styling | `desktop/src/index.css` and `desktop/src/output/theme.ts` |
| Integrate events in app shell | `desktop/src/App.tsx` |
| Regenerate TS contracts | `UPDATE_MODELS=1 cargo test -p runebound-models` |

---

*Last updated: 2026-06-21*  
*Keep this document aligned with rendering contract changes in the same PR.*
