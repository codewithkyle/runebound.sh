# Architecture and Design Patterns

> **Purpose:** This document captures the current, post-refactor architecture and the rules for extending it safely. Read this before changing command routing, entity types, persistence, or rendering contracts.

---

## 1. Workspace Overview

The project is a Rust workspace with a Tauri desktop frontend. Responsibilities are split by crate, and the desktop backend is layered by module boundary:

- `commands/` for command-domain behavior
- `services/` for business workflows and orchestration
- `repositories/` for database and vault access boundaries
- `runebound-models` for shared Rust and TypeScript contracts

### Crates

| Crate | Responsibility | Key Exports |
|---|---|---|
| `core` (`dnd_core`) | Config, database, vault, command parsing, core command execution | `db`, `vault`, `command`, `command_manifest`, `command_parse` |
| `command-handler` | Generic dispatch primitives | `CommandHandler`, `HandlerEntry`, `HandlerRegistry`, `HandlerMetadata` |
| `command-specs` | Command manifest source of truth | `command_manifest()`, `CommandManifest`, `CommandSpec`, `handler_metadata_for()` |
| `runebound-models` | Shared models + TS generation | `NpcDraft`, `LocationDraft`, `FactionDraft`, `ItemDraft`, `OutputDoc`, `events` |
| `wizard` | Host-agnostic multi-step wizard engine, generic over a host `H: WizardHost` | `Wizard`, `WizardStep`, `WizardTransition`, `WizardSession`, `WizardRegistry`, `WizardHost`, `runtime`, `prompt` |
| `desktop/src-tauri` | Desktop command backend | `commands/`, `services/`, `repositories/`, `router.rs`, `main.rs`, `wizards/` (binds the engine to `AppState`) |

---

## 2. Command Dispatch Architecture

The common path (registry dispatch) is:

1. Parse input
2. Normalize aliases/help form
3. Resolve root token via registry
4. Execute handler
5. Return `CommandResponse`

Two routes intentionally diverge from this (override and the generic wizard route); they are described below.

There are two registries using the same `command-handler` crate:

- Core registry in `core/src/command.rs` (`status`, `config`, `help`, `exit`, `setup`, `ping`)
- Desktop registry in `desktop/src-tauri/src/commands/mod.rs` for desktop interaction commands

The desktop registry is consulted first; a miss falls through to the core registry. Two routes bypass plain registry dispatch and must be kept in mind:

- **Desktop overrides core for the same root.** Registering a root in both registries makes the desktop handler win in the desktop app — the supported way to give a core command access to desktop-only state. `help` does this so it can read the open entity editor and active wizard for context-aware output.
- **Generic wizard route (includes onboarding).** While any registered wizard is active (`wizard_session.active_id`), input is routed to `try_execute_active_wizard` (the `wizard` crate's `runtime.rs`, wired in `main.rs` and core's `CommandService`) *before* registry dispatch; onboarding's entry commands launch the same way via `start_wizard`. This is **one** route that serves every wizard, not a per-flow interceptor — adding a wizard adds no dispatch code. The former bespoke onboarding and dungeon interceptors were both deleted in favor of it. See §4's Wizard Framework.

See `docs/command-contexts.md` for the full dispatch-route, context, and parser rules.

### Dispatch Types

```text
CommandSpec (command-specs)
  -> HandlerMetadata
  -> HandlerEntry<Bridge>
  -> HandlerRegistry<Bridge>
```

- `CommandSpec` is declarative and canonical metadata
- `HandlerMetadata` is runtime registry metadata
- `HandlerEntry` binds a name, metadata, and a bridge-backed handler
- `HandlerRegistry` resolves command root to handler

### Router Contract

`desktop/src-tauri/src/router.rs` is dispatch-only:

- If command root exists in desktop registry, invoke handler
- Else, optionally resolve free-form entity references for load/show behavior
- No business logic should be added here

---

## 3. Current Desktop Module Layout

```text
desktop/src-tauri/src/
|- main.rs                  # Tauri command wiring and app startup
|- boot.rs                  # ordered startup tasks (spinner per task) before MOTD
|- router.rs                # registry dispatch + fallback entity resolution
|- app_state.rs             # AppState, EditorSession single-draft `Option<DraftEnvelope>` slot
|- utils.rs                 # shared desktop helpers (tone/topology normalization, …)
|- commands/
|  |- mod.rs                # registry construction + entity_handler_entry + shared response helpers
|  |- create_commands.rs    # create npc|location|faction|item|event|god (+ launches the dungeon/location wizards)
|  |- entity_commands.rs    # generic per-kind verb dispatch (dispatch_entity_command: show|rename|set|travel|reroll|save|cancel) + load|show|preview|delete|undo
|  |- system_commands.rs    # active-kind save|reroll|cancel + the desktop help override
|  |- calendar_commands.rs  # calendar import
|  |- date_commands.rs      # date / date set
|  |- time_delta_commands.rs# +/- relative time deltas
|  |- moon_commands.rs      # moon phases
|  |- publish_commands.rs   # publish entity markdown to the vault
|  `- setup_commands.rs     # setup verbosity/help + native vault folder picker
|- entities/
|  |- mod.rs                # module exports
|  |- kind.rs               # EntityKind + helpers
|  |- schema.rs             # EntityFieldSpec + EntitySchema
|  |- domain.rs             # EntityDomain trait + result helpers
|  |- common.rs             # shared domain helpers/messaging
|  |- registry.rs           # EntityDomainRegistry builder
|  `- domains/              # npc|location|faction|item|event|god|dungeon domain adapters
|- wizards/                 # binds the `wizard` engine crate to AppState
|  |- mod.rs                # impl WizardHost for AppState + build_default_wizard_registry() + re-exports
|  |- dungeon.rs            # the dungeon wizard (linear steps, impl Wizard<AppState>)
|  `- location.rs           # the location wizard (branching create-location flow, impl Wizard<AppState>)
|- repositories/
|  `- mod.rs                # repository traits + Prod* implementations
`- services/
   |- mod.rs                # module exports
   |- ai_generation/        # seed generation, split per entity kind (mirrors entities/domains/)
   |  |- mod.rs             # AiGenerationService + re-export wall (keeps the flat ai_generation::X path)
   |  |- engine.rs          # kind-agnostic core: run_seed_attempts loop, build_seed_payload, SeedSampling table
   |  |- reference.rs       # @reference vault-grounding (build_reference_context, PromptReferenceContext)
   |  `- {npc,location,faction,god,item,event,dungeon}.rs  # per-kind slice: *Seed + generate_*_seed + dedup + tests
   |- entity_reroll.rs      # field reroll generation
   |- entity_persistence.rs # save workflows
   |- entity_persistence_macros.rs # impl_entity_persistence!/impl_entity_soft_delete! per-kind fan-out
   |- entity_admin.rs       # resolve/load/delete/undo/ensure helpers
   |- suggestions.rs        # autocomplete and reference suggestions
   |- vault_sync.rs         # startup vault -> db sync
   |- publish.rs            # entity frontmatter -> Obsidian markdown rendering
   |- ollama_chat.rs        # shared Ollama /api/chat plumbing (generation + reroll)
   |- mention_extraction.rs # Tier-2 LLM link generation for unknown entities
   |- vault_ref.rs          # shared @reference index (AI context + autocomplete)
   `- ts_export.rs          # (test-only) ts-rs export + drift guard for shared models
```

`main.rs` is now thin application wiring, not a command business logic sink.

---

## 4. Entity Domain Architecture

Entities now share one additive architecture:

- **Kinds:** `EntityKind` enumerates every supported entity and exposes helpers (`as_str`, `command_root`, `display_name`). All command/service dispatch takes a kind instead of bespoke enums.
- **Schemas:** `EntitySchema` + `EntityFieldSpec` in `entities/schema.rs` declare canonical fields, aliases, value kinds, and access guards. Validation, suggestions, and help text all consume these specs.
- **Domains:** Each entity implements the `EntityDomain` trait (`entities/domain.rs`) to encapsulate help, show, rename, set, reroll, save, and cancel flows. Domain implementations live under `entities/domains/` and use shared helpers from `entities/common.rs`.
- **Registry:** `EntityDomainRegistry` (`entities/registry.rs`) owns `Arc<dyn EntityDomain>` instances. Command handlers resolve domains by kind, so adding a new entity means registering it once during startup.
- **Editor session:** `app_state.rs`'s `EditorSession` holds at most one draft — a single `Option<DraftEnvelope>` slot (opening, creating, or loading any entity replaces whatever was open, since the frontend only ever renders one card). `active_kind` is derived from the live draft and drives `system save/reroll/cancel`. New entities only need a `DraftEnvelope` variant plus `set_<kind>/get_<kind>/take_<kind>` helpers.

This setup keeps command modules small and makes onboarding new entity types a mostly additive change set: define schema, implement domain, register it, wire persistence/reroll, and expose CLI + frontend hooks.

### Wizard Framework

Multi-step *wizards* (guided flows like `create dungeon` that ask a sequence of questions before producing an artifact) use the same additive, registry-backed pattern as entities — deliberately mirrored so the two read the same way. A wizard is **declarative data plus one trait impl**; the plumbing (dispatch, navigation verbs, clickable prompts, autocomplete context, the spinner signal) lives once and never changes per wizard.

The engine itself is the standalone **`wizard` crate**, host-agnostic and generic over a host type `H: WizardHost` (the host owns the registry + live session and is the context passed to steps). The desktop binds it to `AppState`: `wizards/mod.rs` holds `impl WizardHost for AppState`, the re-exports, and `build_default_wizard_registry()`, and `wizards/dungeon.rs` + `wizards/location.rs` are the concrete desktop wizards. This split is what lets core/CLI reuse the same engine — onboarding runs on it via `core/src/onboarding_wizard.rs`.

| Entity domain (one-shot create) | Wizard (multi-step flow) |
|---|---|
| `EntityKind` variant | stable `id()` string (`"dungeon"`) |
| `EntitySchema` + `EntityFieldSpec` | ordered `WizardStep`s (declarative) |
| `EntityDomain` trait (`entities/domain.rs`) | `Wizard` trait (`wizard` crate, `wizard.rs`) |
| `EntityDomainRegistry` + `build_default_registry()` | `WizardRegistry` + `build_default_wizard_registry()` |
| `EditorSession` (`active_kind` + single-draft `Option<DraftEnvelope>` slot) | `WizardSession` (`active_id` + cursor + history + type-erased data) |
| `InputContext::EntityEditor(kind)` | `InputContext::Wizard(id)` |
| `CommandAvailability::EntityScoped` | `CommandAvailability::AnyWizard` (`continue`/`back`) + `AnyEditorOrWizard` (`cancel`) |
| Card footer emits `command_ref` (`drafts.rs`) | Step prompt emits `command_ref` **by construction** (`wizard` crate, `prompt.rs`) |

Key pieces (all in the `wizard` crate unless noted):

- **`Wizard<H>` + `WizardStep<H>` traits** (`wizard.rs`): a wizard exposes `id/title/steps/seed(host)/finalize`; each step exposes `prompt/summary/choices/suggest/awaiting_llm_label/accept`. `accept()` returns a `WizardTransition` (`Stay`/`Next`/`Goto(id)`/`Back`/`Complete`/`Cancel`/`Native(action)`) and is — with `finalize()` and `seed(host)` — the *only* host-coupled surface (it takes `&H`, the host context, e.g. `&AppState`); everything else is host-agnostic, which is why the engine is its own crate. `seed(host)` builds the initial accumulator and can pre-fill from host/effective config (used by the onboarding port). `summary()` feeds the in-wizard `help`; `suggest(input, data)` defaults to prefix-filtering `choices()` and is overridden for staged multi-token args (`set room <room> <type>`).
- **Native capabilities** (`WizardHost::perform_native`): a step that needs a host-only side effect (e.g. a folder picker) returns `WizardTransition::Native(NativeAction::…)`; the engine calls `perform_native`, then on `Provided(value)` resubmits the value to the action's target step and on `Cancelled` re-renders the requesting step. The default `perform_native` returns `Cancelled`, which is the graceful headless/CLI degradation path. This keeps the engine host-agnostic while letting a (core-defined) step drive a desktop dialog.
- **`WizardSession`** (`session.rs`): the live cursor, a history stack that powers `back`, and a type-erased `WizardData` accumulator (`Box<dyn Any>`, the wizard analogue of `DraftEnvelope`). Lives on the host (`AppState`) next to `wizards: Arc<WizardRegistry<AppState>>`; the `WizardHost` trait exposes both to the engine.
- **One dispatch route** (`runtime.rs`): `try_execute_active_wizard` handles the global verbs (`cancel`/`back`/`help` — `help` renders the step's commands without advancing), delegates to the active step's `accept()`, applies the transition, and renders the next prompt (or runs `finalize`). It populates the structured `CommandResponse.wizard: WizardView { id, step_id, awaiting_llm_label }` that drives the frontend spinner — no prompt-text matching.
- **Clickability by construction** (`prompt.rs`): the sanctioned prompt builders (`wizard_menu`, `action_row`, `choice_lines`) render every `WizardChoice` as a `command_ref`, so an author *cannot* emit a non-clickable choice.
- **Autocomplete for free**: `resolve_input_context` returns `InputContext::Wizard(id)` while a wizard is active; the suggestion service then early-returns `active_step_suggestions`, which combines the step's `suggest()` (per-step tokens + staged args) with the always-available global verbs (`back`/`cancel`/`help`). The step owns the whole command surface, so only the commands valid *here* are offered.

The dungeon wizard (desktop `wizards/dungeon.rs`, `impl Wizard<AppState>`) is the simplest reference implementation — a linear sequence of steps. The location wizard (`wizards/location.rs`) is the reference for a *branching* flow: step 1 picks the `kind_type`, which routes to one of several branches (settlement/site/hideout/guildhall/custom) via `WizardTransition::Goto`. See §8D for the "Add a New Wizard" playbook.

### Combining the two: a kind can be wizard-created *and* entity-edited

The wizard and entity-domain patterns are **complementary, not mutually exclusive** — and a single entity kind can use both. A wizard's `finalize()` can hand its artifact straight into an entity draft, so the kind is *created* through a guided multi-step flow and then *edited* with the ordinary one-shot entity commands.

**Dungeon and Location are the live examples of both at once.** `create dungeon` launches `DungeonWizard` (registered in `build_default_wizard_registry()`); its `finalize()` generates the dungeon and then calls `editor.set_dungeon(draft)` — the exact hand-off a one-shot create handler performs. From that moment the kind behaves like any other entity: the live context flips from `InputContext::Wizard("dungeon")` to `InputContext::EntityEditor("dungeon")` (the entity editor wins precedence — see `docs/command-contexts.md` §1; the wizard session resets on `Complete`), and the full `DungeonDomain` takes over — `dungeon show|rename|set|reroll|save|cancel`. So `EntityKind::Dungeon`, `DungeonDraft` + `dungeon_entity_card` (`drafts.rs`), and the generic entity dispatch for the `dungeon` root coexist with `DungeonWizard`; the two halves meet at `finalize()`.

**Location does the same, with a twist:** bare `create location` launches `LocationWizard`, whose `finalize()` calls `editor.set_location(draft)` and hands off to the `LocationDomain` editor — but `create location <prompt>` keeps the original one-shot lane (no wizard). So a single kind can offer a guided flow *and* a quick one-shot create, both converging on the same `LocationDraft` and entity editor. (Dungeon has no one-shot lane: both `create dungeon` and `create dungeon <args>` launch the wizard.)

**Which to reach for:**

| Need | Pattern |
|---|---|
| Creation needs a guided sequence (several questions, LLM steps, branching), result is *not* further edited | Wizard only (e.g. onboarding — `finalize` writes config, no entity draft) |
| Creation is a single prompt, result is editable afterward | Entity domain only (e.g. `create npc`) |
| Creation needs a guided sequence **and** the result stays editable | **Both** — a `Wizard` for the create path whose `finalize()` opens an `EntityDomain` draft for the edit path (the dungeon model) |

If you build a kind that needs both, follow §8D for the wizard and §8C for the entity domain, and have the wizard's `finalize()` call the same `editor.set_<kind>(draft)` hand-off the create handler would.

## 5. Command Manifest and Metadata Rules

The manifest in `command-specs/src/lib.rs` is the single source of truth for:

- command names, subcommands, examples
- aliases
- execution target (`Core` or `Desktop`)
- autocomplete visibility (`show_in_autocomplete`)
- subcommand requirement (`requires_subcommand` — drives the parser's argument-vs-subcommand decision)
- canonical help command for clickability

The same file also owns **context availability** via `command_availability(name)` and the `InputContext`/`CommandAvailability` enums. This is the single source of truth for which commands appear in which context (default surface, entity editor, active wizard), consumed by both autocomplete and the help index. Adding a command without an explicit availability arm leaves it `Default`-only (invisible in editors) — a common regression. The parser's `requires_subcommand` semantics and the help↔autocomplete parity are documented in `docs/command-contexts.md`; read it before changing visibility, parsing, or help.

Manifest data is consumed by:

- backend suggestion service (`desktop/src-tauri/src/services/suggestions.rs`)
- help renderers in `core/src/command.rs`
- desktop/core registry metadata generation
- frontend command clickability fallbacks

If you rename or add command tokens without updating manifest entries, help/autocomplete/clickability will drift.

---

## 6. Repository and Service Boundaries

### Repository Rules

Use repositories from `AppState` for all DB/vault operations in command/service code.

- Allowed: `state.npc_repo().find_by_name_or_slug(...)`
- Not allowed: direct `core::db::*` calls from handlers

### Service Rules

Handlers orchestrate; services implement workflows.

- `AiGenerationService` handles seed generation
- `EntityRerollService` handles field rerolls
- `EntityPersistenceService` handles save + write + index upsert paths
- `EntityAdminService` handles entity resolve/load/delete/undo and ensure-location flows
- `SuggestionService` handles autocomplete and reference suggestions
- `VaultSyncService` projects the canonical TOML store into the db + document index at startup (store → db only; the Obsidian `.md` vault is a publish target, not an input)

Alongside these orchestrating `*Service` types, `services/` also holds shared support modules (free functions, not `Service` structs): `publish.rs` (entity frontmatter → Obsidian markdown), `ollama_chat.rs` (shared Ollama `/api/chat` plumbing for generation + reroll), `vault_ref.rs` (the `@reference` index used by AI context + autocomplete), and `mention_extraction.rs` (Tier-2 LLM link generation for not-yet-known entities).

**`ai_generation` is itself a per-kind module, not one file.** `services/ai_generation/` mirrors `entities/domains/`: `engine.rs` holds the kind-agnostic core (the `run_seed_attempts` retry loop, the pure `build_seed_payload`, the per-kind `SeedSampling` table, token/capacity math), `reference.rs` holds the `@reference` vault-grounding subsystem, and one file per kind (`npc.rs`, `location.rs`, …) holds that kind's `*Seed` DTO, its `generate_*_seed` method (an `impl AiGenerationService` block), its dedup/recency helpers, and its tests. `mod.rs` is a thin re-export wall that preserves the flat `crate::services::ai_generation::X` path so consumers never change. **Adding a kind means adding a file, not growing one:** declare `mod <kind>;`, add the slice, add a `<KIND>_GEN_SAMPLING` const in `engine.rs`, and call the shared `run_seed_attempts` rather than re-inlining the loop. Do not collapse this back into a single file — the "restructured LLM generation" split is exactly what this guards.

Use command modules for command syntax and user-facing response behavior. Use services for heavy domain logic.

---

## 7. Shared Models and Contracts

`runebound-models` is the cross-layer contract for:

- editor drafts
- output documents and inline nodes
- command events and command responses

### Rule: Model First

When introducing new domain concepts used by both backend and frontend:

1. Add Rust model in `runebound-models/src/*`
2. Ensure `build.rs` exports TS type
3. Regenerate via `cargo build -p runebound-models`
4. Consume generated TS model in frontend

Do not define parallel, hand-rolled TS interfaces for model concepts already in `runebound-models`.

---

## 8. Extension Playbooks

### A) Add New Top-Level Command

1. Add `CommandSpec` in `command-specs/src/lib.rs` (set `requires_subcommand` correctly: `false` if it takes a free-form argument such as a name/value, even when it also has a `help` subcommand)
2. Add a `command_availability` arm in `command-specs/src/lib.rs` unless the command is genuinely default-surface-only (the `_ => Default` fallthrough hides it from every editor context)
3. Implement handler in `desktop/src-tauri/src/commands/<domain>_commands.rs` (or `core/src/command.rs` for core)
4. Register handler entry in `desktop/src-tauri/src/commands/mod.rs` (or core registry)
5. Verify help and autocomplete in **each** context the command should appear in (default, entity editor, setup), plus clickability paths

No router changes are needed for normal top-level command additions. See `docs/command-contexts.md` for availability and parser rules.

### B) Add New Subcommand

1. Add subcommand metadata in `command-specs/src/lib.rs`
2. Implement syntax and behavior in the domain command module
3. Update suggestion behavior if field/value completion is expected
4. Add/verify phrase help output

### C) Add New Entity Type (example: `item`, `quest`, `dungeon`)

After the v0.5.0 unification, the per-entity fan-out is generated from the schema +
a macro per layer, so an entity is mostly **one declaration per layer** rather than
the ~250–550 hand-written lines it used to cost. No per-field branching.

1. **Kind + schema**: add the `EntityKind` variant to `ALL_ENTITY_KINDS` and its
   `EntitySchema` (field specs: `value_kind`, `settable`/`rerollable`,
   `reroll_instruction`) in `entities/{kind,schema}.rs`. The schema drives `set`,
   `reroll`, and the entity card.
2. **Model**: add the `*Draft` + `*Frontmatter` + `*_entity_card` in
   `runebound-models` (derive `TS` on the draft), then regenerate `models.ts`
   (`UPDATE_MODELS=1 cargo test -p runebound-models`).
3. **Domain**: implement `<Entity>Domain` under `entities/domains/` and register it in
   the domain registry; most behavior comes from `EntityDomain` defaults.
4. **DB**: add the migration + `*Row` struct, one `impl_entity_table!` column
   declaration in `core/src/db.rs` (generates the whole CRUD set), and the repository
   trait + prod impl (`upsert_tx`/`delete_by_id_tx` + reads).
5. **Generation / persistence / soft-delete / reroll**: add a
   `services/ai_generation/<kind>.rs` slice for the create path (the `*Seed` DTO, a
   `generate_<kind>_seed` that calls the shared `run_seed_attempts` loop, a
   `<KIND>_GEN_SAMPLING` const in `engine.rs`, and `mod <kind>;` in
   `ai_generation/mod.rs` — see §6). Then one `impl_entity_persistence!` and one
   `impl_entity_soft_delete!` declaration, plus the reroll spec entries;
   `vault_sync`'s `*_row_from_frontmatter` projects the canonical store row.
6. **Draft slot + command**: add the `DraftEnvelope::<Entity>` variant (the editor is
   a single-draft slot) and register `entity_handler_entry(root, kind)` in
   `commands/mod.rs` — the generic `dispatch_entity_command` runs the verb ladder off
   `command_root()` + the schema (no per-kind module, no magic offsets).
7. **Verify**: `cargo test --workspace` + the desktop suite (the schema/registry
   contract tests guard the wiring), then walk create → show → set → reroll → save →
   load → delete → undo for the new kind.

### D) Add a New Wizard (multi-step guided flow)

Mirrors C, but for a sequence of prompts rather than a one-shot create. The plumbing is already written; a new wizard is additive data + one trait impl (see §4 Wizard Framework).

1. **Steps + data**: in a new `wizards/<name>.rs`, define the accumulator struct and one `WizardStep<AppState>` impl per step. Build prompts only with the `wizard` crate's `prompt.rs` helpers (`wizard_menu`/`action_row`/`choice_lines`) so every choice is a clickable `command_ref`. Set `awaiting_llm_label()` on any step whose submission calls the LLM.
2. **Wizard impl**: implement the `Wizard` trait (`id/title/steps/seed/finalize`). `finalize()` builds the artifact and hands off (open an entity draft, write config, …) exactly as a create handler would.
3. **Register**: add one line in `build_default_wizard_registry()` (`wizards/mod.rs`).
4. **Launch**: point the entry command at `start_wizard("<id>", state)` (mirror `create dungeon` in `create_commands.rs`).
5. **No plumbing edits**: dispatch (`try_execute_active_wizard`), the nav verbs, `InputContext::Wizard`, the spinner signal, and step-token autocomplete all work without changes. The nav verbs already have `command_availability` arms (`continue`/`back` = `AnyWizard`, `cancel` = `AnyEditorOrWizard`).
6. **Verify**: `cargo test suggestions` (step-token typeahead), then manually walk the flow — every step's choices clickable, `back`/`cancel` at each step, spinner on LLM steps, `finalize` produces the artifact.

The only edits outside `wizards/<name>.rs` are the one registry line and the launch call. If a wizard needs a new step *capability* (a new `WizardTransition`, a config seed), that is an engine change in the `wizard` crate shared by all wizards (the onboarding wizards in `core/src/onboarding_wizard.rs` are the reference for engine-level seed/native extensions).

**Authoring conventions (faction/location are the references; each is test-enforced):**

- **Step ids are `const`s, not bare strings.** Declare one `const STEP_*: &str` per step (plus `const MODE_*` for picker routing keys that aren't step ids) and use them for both `id()` and every `Goto`/`enter_*` target, so a typo or rename is a compile error. A `every_declared_step_id_is_registered_once` test asserts the declared id set equals the registered set — which, since every `Goto` uses a const, proves no route dangles (the engine otherwise rejects an unknown `Goto` only at runtime).
- **`Next` couples to declaration order; `Goto` does not.** `Next` is `cursor += 1`, so an intra-branch `Next`-chain only works while its steps stay contiguous and ordered in the `steps()` Vec. Either route every step with `Goto` (faction) or add a contiguity test pinning the adjacencies (location's `next_chains_stay_contiguous_in_declared_order`).
- **`back` rolls back the cursor, not the accumulator.** A step that parks transient sub-state in the accumulator (a mode flag, an "awaiting custom input" toggle, an accumulating list) must reset it when the step is (re)entered — do it in the `enter_*` helper or at the inbound `Goto` site, never by assuming `back` clears it.
- **Multiplex a picker over one entity set; split a repeatable accumulator out.** One step may serve several single-value link points via a mode field (the faction picker: liege/patron). But a *repeatable* picker that accumulates a list (allies→rivals) gets its **own** step and its **own** mode field, so its `Stay`-loop and "linked so far" state never entangle with single-value modes.
- **Lift pure step helpers into `wizard::prompt`, don't copy them.** `numbered_choices`/`pick_value`/`skip_choice`/`optional_text`/`optional_text_prompt` and `weak_random_index` live in the shared crate; menu `*_LABELS` arrays that pair with a canonical `*_VALUES`/vocab array carry an alignment test (length + a representative index) so a reorder can't silently mislabel a menu.

---

## 9. Anti-Patterns

| Anti-Pattern | Why It Is Wrong | Correct Approach |
|---|---|---|
| Command business logic in `router.rs` | Breaks dispatch-only router contract | Put behavior in `commands/*.rs` and services |
| Direct `core::db` calls from handlers | Bypasses testable boundaries | Use repositories from `AppState` |
| Duplicated cross-layer types | Causes drift between Rust and TS | Use `runebound-models` first |
| Large, ad-hoc parsing in frontend for command semantics | Duplicates backend command rules | Keep parser authority backend-first |
| Depending on markdown heuristics for command links | Fragile clickability | Emit explicit `command_ref` nodes |
| Hard-coding per-command visibility in a surface | Drifts from the real availability and from other surfaces | Ask `command_availability(name)` — the single source of truth |
| Relying on context to "block" a command from running | Contexts only filter help/autocomplete, not execution | Guard inside the handler if a command must be refused |
| New command left on the `_ => Default` availability arm | Silently hidden in every editor context | Add an explicit `command_availability` arm |
| A bespoke per-flow interceptor in `main.rs` for a multi-step flow | Re-creates the dungeon-flow drift (no context, no typeahead, hand-built prompts) | Register a `Wizard`; the generic route, context, and clickable prompts come for free (§4) |
| Re-inlining the seed retry loop or putting a new kind's generation in an existing `ai_generation` file | Re-monolithizes the file the split just broke up | New kind = a new `ai_generation/<kind>.rs` slice that calls the shared `run_seed_attempts` (§6, §8C) |

---

## 10. Known Friction Points

The v0.5.0 cleanup (archived at `docs/archive/cleanup-0.5.0.md`) resolved the friction
points this section used to list:

- **Per-entity command duplication** — the seven near-identical entity command modules
  collapsed into one generic `dispatch_entity_command` driven by `command_root()` + the
  schema (P5.3).
- **Large entity-fan-out services** — `entity_admin`/`entity_persistence`/`entity_reroll`
  dispatch through the `EntityDomain` registry and per-entity macros
  (`impl_entity_table!`, `impl_entity_persistence!`, `impl_entity_soft_delete!`), so a new
  entity is one declaration per layer rather than hundreds of lines (P5.2/5.5/5.6/5.7).
- **Explicit type branches per entity** — driven by `ALL_ENTITY_KINDS` + the schema; see
  the (now short) §8.C playbook.

Also closed by the cleanup: the markdown-heuristic clickability and dual help/text
renderers (now one backend-authored `OutputDoc` + `to_plain_text`, P3/P7.2); the
Rust↔TS contract drift (ts-rs single-sourced + drift-guarded, P4); non-atomic
persistence (a `Database::begin()` transaction wraps save/soft-delete/reap, P6.1); and
the wizard's cross-field `data` invariant (now a type-state enum, P7.4).

Remaining, deliberately-deferred follow-ups (each noted in-code):

- the wizard restore/undo + `ensure_location_exists` upsert paths are not yet
  transactional (recovery-side, lower-risk — P6.1 covers save/soft-delete/reap);
- `EntityStore` reads on the startup/save paths stay synchronous (not the per-keystroke
  path — P6.2 moved the `@reference` scan off the runtime);
- `ai_generation`'s create-path sampling values remain hardcoded consts — the
  "restructured LLM generation" change centralized them into the per-kind
  `*_GEN_SAMPLING` table in `ai_generation/engine.rs` (resolving the inline-literal
  duplication P2.5 had left in the create half), but they are still not config-exposed;
- `vault_sync` projects the canonical store → db only; the Obsidian `.md` vault is a
  publish target, not an input (P6.3 — a true disk-scan half was intentionally not built).

If entity count grows further, the schema/registry is the lever — extend the field spec,
not the fan-out.

---

## 11. Feature Development Checklist

Before merging any feature that changes commands/entities:

- [ ] Manifest updates complete in `command-specs/src/lib.rs` (including `requires_subcommand` and a `command_availability` arm)
- [ ] Handler implementation placed in correct command domain module
- [ ] Registry registration updated
- [ ] Help and autocomplete verified in every context the command should appear in (default / entity editor / setup)
- [ ] Repository/service boundaries respected (no direct DB from handlers)
- [ ] `output_doc` and `command_ref` used for actionable output
- [ ] Frontend model usage comes from generated `desktop/src/generated/models.ts`
- [ ] Suggestions updated when command surface or fields changed
- [ ] `make build` passes
- [ ] Primary user flow manually verified (help, run, save/cancel, load/show where applicable)

---

## 12. Related Docs

- `docs/command-contexts.md` for command contexts, availability, parser semantics, and the setup-wizard dispatch route
- `docs/cli.md` for command UX contracts and command implementation checklist
- `docs/render.md` for output rendering rules and card/output extension guidance
- `docs/feature-development.md` for end-to-end implementation playbooks

---

*Last updated: 2026-06-20*  
*If this document drifts from the codebase, update it in the same PR as the architecture change.*
