# Feature Development Playbook

> **Purpose:** This is the practical implementation guide for building new features on the refactored architecture.

---

## 1. Start Here: Layer Responsibilities

- `command-specs` defines command metadata and aliases
- `commands/*.rs` handles command syntax and user-facing responses
- `services/*.rs` handles workflows and domain logic
- `repositories/mod.rs` handles database and vault boundaries
- `runebound-models` defines shared Rust and TS contracts
- `desktop/src/App.tsx` integrates frontend events and rendering
- Canonical entity data is stored as TOML under `~/.config/runebound.sh/entities/<kind>/<slug>.toml`; Obsidian files are generated via `publish`

If your change crosses backend/frontend boundaries, model it in `runebound-models` first.

---

## 2. Playbook A: Add a New Top-Level Command

Example: add `quest`.

1. Add command metadata in `command-specs/src/lib.rs` (set `requires_subcommand` — `false` if it takes a free-form argument like a name/value)
2. Add a `command_availability` arm in `command-specs/src/lib.rs` unless the command is genuinely default-surface-only (the `_ => Default` fallthrough hides it from every editor context)
3. Create `desktop/src-tauri/src/commands/quest_commands.rs`
4. Add `quest_handler_entry()` in `desktop/src-tauri/src/commands/mod.rs`
5. Register it in `build_desktop_handler_registry()`
6. Add structured response output and command refs
7. Update suggestion behavior only if command has custom argument completions
8. Verify `help` and autocomplete in every context the command should appear in (default / entity editor / setup)

Notes:

- do not add command logic in `router.rs`
- no `main.rs` command branching
- visibility is data, not code: it comes from `command_availability` (see `docs/command-contexts.md`), not hand-written filters

---

## 3. Playbook B: Add a New Subcommand

1. Add subcommand metadata under the parent `CommandSpec`
2. Implement behavior in the existing domain handler module
3. If the subcommand changes field names or argument completion, update `services/suggestions.rs`:
   - add the field to the appropriate schema + `settable_fields`/`rerollable_fields`
   - ensure `entity_kind_for_root()` returns the expected `EntityKind`
   - extend command filtering so system commands (`save`, `reroll`, `cancel`) stay visible for the correct drafts
   - add/refresh suggestion tests (`cargo test suggestions`)
4. Add explicit usage and help output paths

Validation focus:

- `help <command>` and `<command> help`
- autocomplete visibility
- clickability for suggested next actions

---

## 4. Playbook C: Add a New Entity Type

Example entity classes: `item`, `dungeon`, `quest`.

### Entity schema + domain

1. Add the new `EntityKind` variant and schema constants in `desktop/src-tauri/src/entities/{kind,schema}.rs`.
2. Implement `<Entity>Domain` under `entities/domains/`, using helpers from `entities/common.rs` for consistent messaging.
3. Register the domain inside `build_default_registry()` so command handlers and system commands can resolve it.
4. Extend `DraftEnvelope` and the `EditorSession` helpers in `app_state.rs` with `get_/set_/take_` methods for the new draft type.

### Backend data and persistence

1. Add DB schema and CRUD in `core/src/db.rs` (+ migration)
2. Add repository trait + production implementation in `desktop/src-tauri/src/repositories/mod.rs`
3. Add persistence workflow in `desktop/src-tauri/src/services/entity_persistence.rs`
4. Add reroll + AI-generation helpers in `desktop/src-tauri/src/services/entity_reroll.rs` (and `services/ai_generation.rs` if seed prompts change)
5. Add admin resolution/load/delete/undo support in `desktop/src-tauri/src/services/entity_admin.rs`
6. Wire the canonical `EntityStore` sync in `desktop/src-tauri/src/services/vault_sync.rs` so DB + search indexes mirror the TOML records (no Markdown parsing)

### Commands and editor state

1. Add draft/frontmatter model + card builder in `runebound-models/src/drafts.rs`
2. Add create/edit command module under `desktop/src-tauri/src/commands/` (mirror `item_commands.rs` for expected UX)
3. Register handler in `desktop/src-tauri/src/commands/mod.rs`
4. Extend shared entity actions (`load/show/preview/delete/undo`) in `desktop/src-tauri/src/commands/entity_commands.rs`
5. Update `services/suggestions.rs` so autocomplete filters, field lists, and entity search handle the new type. Confirm all of the following:
   - `entity_kind_for_root()` returns the new kind
   - `build_entity_field_argument_suggestions()` pulls from the new schema via `settable_fields`/`rerollable_fields`
   - System commands (`save`, `reroll`, `cancel`) remain visible whenever any draft of the new kind is active
   - Tests cover the new root/subcommand completions (`cargo test suggestions`)

### Frontend integration

1. Extend events/types in `runebound-models/src/events.rs` if needed
2. Regenerate TS models (`cargo build -p runebound-models`)
3. Handle new event/draft pathways in `desktop/src/App.tsx`
4. Add spinner labels in `commandSpinnerLabel()` (`desktop/src/App.tsx`) for the new kind's LLM-backed commands — `create <kind>`, `<kind> reroll`, and `<kind> save` (see §8). The generation/reroll commands call the LLM and MUST show a spinner.
5. Verify card rendering through existing `OutputRenderer` path
6. Confirm client events (draft load, clear, etc.) trigger the desired UI flows

---

## 5. Playbook D: Add New Card/Output UI

### New custom card for existing/new entity

1. Build card in `runebound-models/src/drafts.rs`
2. Return card via `CommandClientEvent`
3. Ensure app integration branch exists in `desktop/src/App.tsx`
4. Prefer generic `entity_card` block unless a genuinely new block type is needed

### New output block type

1. Add `OutputBlock` variant in `runebound-models/src/output.rs`
2. Regenerate TS
3. Render variant in `desktop/src/output/renderer.tsx`
4. Add styles in `desktop/src/index.css`
5. Add fallback parse behavior only if plain-text compatibility matters

---

## 6. Playbook E: Add New Suggestion Behavior

1. Keep parser authority backend-first (`core/src/command_parse.rs`). Do not duplicate parse rules in the frontend.
2. Update `desktop/src-tauri/src/services/suggestions.rs` in all relevant stages:
   - **Context visibility:** gating is driven by `command_availability(name)` against the resolved `InputContext` — adjust the availability arm in `command-specs/src/lib.rs`, do not add a bespoke per-command filter here. The help index uses the same source, so both surfaces stay in sync (`docs/command-contexts.md`).
   - **Root stage:** ensure new roots have manifest coverage and appear unless intentionally hidden.
   - **Subcommand stage:** confirm `find_command()` + manifest data expose the subcommand.
   - **Argument stage:** update `build_entity_field_argument_suggestions()` (or a new helper) so field lists remain in sync with schemas. Always update `entity_kind_for_root()` when adding a new entity root.
   - **System filters:** verify that `save`, `reroll`, and `cancel` stay visible whenever any draft is active. Filters should only hide these commands when `active_kind` is `None`.
3. Add or update `#[cfg(test)]` coverage in `services/suggestions.rs` for every new root/subcommand/field completion. The guideline is “every new suggestion path gets a test.”
4. Run `cargo test suggestions` (from `desktop/src-tauri`) after any change to autocomplete logic.
5. Verify frontend rendering without duplicating semantics in the frontend parser; the UI should simply display the backend-provided suggestions.

---

## 7. Playbook F: Add a New Wizard

A *wizard* is a guided multi-step flow (ask a sequence of questions, then build an artifact) — `create dungeon` is the reference. The engine in `desktop/src-tauri/src/wizards/` owns dispatch, the nav verbs, clickable prompts, autocomplete, and the spinner signal; a new wizard is additive data + one trait impl. See `docs/architecture.md` §4 (Wizard Framework) and §8D.

### Steps + wizard

1. Create `wizards/<name>.rs`. Define the accumulator struct (the per-flow answers; the cursor/history are engine-owned in `WizardSession`).
2. Implement one `WizardStep<AppState>` per step (`id`/`prompt`/`choices`/`awaiting_llm_label`/`accept`):
   - Build prompts **only** with the `wizard` crate's `prompt.rs` helpers (`wizard_menu`/`action_row`/`choice_lines`) so every `WizardChoice` renders as a clickable `command_ref`. Never hand-build a prompt with back-tick text.
   - `accept()` validates the input and returns a `WizardTransition` (`Stay`/`Next`/`Goto(id)`/`Back`/`Complete`/`Cancel`); it and `finalize()` are the only places `&AppState` is touched.
   - Set `awaiting_llm_label()` (e.g. `"generating story"`) on any step whose submission calls the LLM — this drives the spinner with no frontend text-matching (see §8).
3. Implement the `Wizard<AppState>` trait (`id`/`title`/`steps`/`seed`/`finalize`). `finalize()` builds the artifact and hands off (open an entity draft, write config, …) exactly like a one-shot create handler.

### Wiring

4. Register the wizard with one line in `build_default_wizard_registry()` (`wizards/mod.rs`).
5. Point the entry command at `start_wizard("<id>", state)` (mirror `create dungeon` in `commands/create_commands.rs`).
6. **No plumbing edits.** Dispatch, `InputContext::Wizard`, the global verbs (`continue`/`back`/`cancel` + the in-wizard `help`), step typeahead (`active_step_suggestions` = the step's `suggest()` + globals), and the `WizardView` spinner signal all work unchanged. Give each step a `summary()` (for `help`) and `with_help(...)` on its choices; override `suggest()` only for staged multi-token args. If a wizard needs a brand-new *capability*, that is a shared engine change in `wizards/`, not per-wizard code (`docs/onboarding-wizard-port.md` tracks the planned extensions).

### Verify

7. `cargo test suggestions` (step-token typeahead), then walk the flow manually: every step's choices clickable, `back`/`cancel` at each step, the spinner on LLM-backed steps, and `finalize` producing the artifact.

---

## 8. Output and UX Standards

- Prefer `output_doc` for non-trivial output
- Use `command_ref` for actionable text
- Keep usage/help copy concise and stable
- Reject `-h` and `--help`; phrase help only
- Keep keyboard UX stable (`Enter`, `Tab`, arrows, `Ctrl+C`)
- **Any command that calls the LLM MUST show a spinner.** LLM/Ollama round-trips
  (entity generation, per-field and whole-draft reroll, seed generation, model
  probes) are slow enough that a command with no feedback reads as a hang. The
  spinner is frontend-driven: add a pattern for the command in
  `commandSpinnerLabel()` in `desktop/src/App.tsx` (the single source of truth —
  e.g. `"generating <kind>"`, `"rerolling <kind>"`). If you add a new
  LLM-backed command or entity kind and skip this, the call will appear to do
  nothing until it returns. This is the gap the "missing events spinner" fix
  closed; treat a spinner as part of the feature, not a follow-up.
  - **Wizard steps are the exception to the pattern, not the rule:** a step whose
    submission calls the LLM sets `awaiting_llm_label()`, which rides the structured
    `WizardView` signal so the frontend shows the spinner without any
    `commandSpinnerLabel()` text-matching. Set the label; don't add a per-command
    pattern for wizard verbs.

---

## 9. Anti-Patterns to Avoid

- command logic in `router.rs` or `main.rs`
- direct DB access from command handlers
- duplicate Rust/TS model definitions
- frontend-only command semantics
- markdown heuristic dependence for new feature clickability

---

## 10. Definition of Done Checklist

Use this for every feature PR:

- [ ] architecture layer boundaries respected
- [ ] manifest updated for all command surface changes (including `requires_subcommand` and a `command_availability` arm)
- [ ] handlers implemented and registered
- [ ] help + autocomplete verified in every relevant context (default / entity editor / setup)
- [ ] repository + service paths updated where persistence/workflows changed
- [ ] output uses `output_doc` and explicit `command_ref` for actions
- [ ] every LLM-backed command has a spinner via `commandSpinnerLabel()` (`desktop/src/App.tsx`)
- [ ] shared models updated in `runebound-models` and TS regenerated
- [ ] frontend integration updated (`App.tsx`, renderer/theme/css if needed)
- [ ] autocomplete and mode filtering validated (`cargo test suggestions`)
- [ ] core command help and phrase-help behavior validated
- [ ] `make build` passes
- [ ] primary user flows manually exercised

---

## 11. Suggested PR Verification Commands

```bash
cargo build -p runebound-models
make build
```

Then manually test the affected command flows in desktop UI:

- create/load/show/edit/save/cancel/reroll as applicable
- help and autocomplete coverage
- clickable output actions

---

## 12. Related Docs

- `docs/architecture.md`
- `docs/cli.md`
- `docs/command-contexts.md`
- `docs/render.md`

---

*Last updated: 2026-06-15*
