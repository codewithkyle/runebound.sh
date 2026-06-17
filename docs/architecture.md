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
| `desktop/src-tauri` | Desktop command backend | `commands/`, `services/`, `repositories/`, `router.rs`, `main.rs` |

---

## 2. Command Dispatch Architecture

The common path (registry dispatch) is:

1. Parse input
2. Normalize aliases/help form
3. Resolve root token via registry
4. Execute handler
5. Return `CommandResponse`

Three routes intentionally diverge from this (override, onboarding interception, and the generic wizard route); they are described below.

There are two registries using the same `command-handler` crate:

- Core registry in `core/src/command.rs` (`status`, `config`, `help`, `exit`, `setup`, `ping`)
- Desktop registry in `desktop/src-tauri/src/commands/mod.rs` for desktop interaction commands

The desktop registry is consulted first; a miss falls through to the core registry. Three routes bypass plain registry dispatch and must be kept in mind:

- **Desktop overrides core for the same root.** Registering a root in both registries makes the desktop handler win in the desktop app — the supported way to give a core command access to desktop-only state. `help` does this so it can read the open entity editor for context-aware output.
- **Onboarding interception.** While the setup wizard is active (`onboarding.active`), input is routed to `try_execute_onboarding` *before* registry dispatch, so the desktop registry is bypassed during setup.
- **Generic wizard route.** While any registered wizard is active (`wizard_session.active_id`), input is routed to `try_execute_active_wizard` (`wizards/runtime.rs`) *before* registry dispatch. This is **one** route that serves every wizard, not a per-flow interceptor — adding a wizard adds no dispatch code. See §4's Wizard Framework.

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
|- main.rs                 # Tauri command wiring and app startup
|- router.rs               # registry dispatch + fallback entity resolution
|- app_state.rs            # AppState, EditorSession map-backed DraftEnvelope store
|- commands/
|  |- mod.rs               # registry construction + shared response helpers
|  |- create_commands.rs   # create npc|location|faction|item
|  |- npc_commands.rs      # npc show|rename|set|travel|reroll|save|cancel
|  |- location_commands.rs # location show|rename|set|reroll|save|cancel
|  |- faction_commands.rs  # faction show|rename|set|reroll|save|cancel
|  |- item_commands.rs     # item show|rename|set|reroll|save|cancel
|  |- entity_commands.rs   # load|show|preview|delete|undo
|  `- system_commands.rs   # active-kind save|reroll|cancel
|- entities/
|  |- kind.rs              # EntityKind + helpers
|  |- schema.rs            # EntityFieldSpec + EntitySchema
|  |- domain.rs            # EntityDomain trait + result helpers
|  |- registry.rs          # EntityDomainRegistry builder
|  `- domains/             # npc|location|faction|item domain adapters
|- wizards/                # multi-step wizard engine (mirrors entities/)
|  |- wizard.rs            # Wizard + WizardStep traits, WizardChoice/Transition
|  |- session.rs           # WizardSession (cursor + history + type-erased data)
|  |- registry.rs          # WizardRegistry + build_default_wizard_registry()
|  |- runtime.rs           # try_execute_active_wizard(): the single dispatch route
|  |- prompt.rs            # wizard_menu/action_row: clickable prompts by construction
|  `- dungeon.rs           # the dungeon wizard (declarative steps)
|- repositories/
|  `- mod.rs               # repository traits + Prod* implementations
`- services/
   |- ai_generation.rs     # seed generation
   |- entity_reroll.rs     # field reroll generation
   |- entity_persistence.rs# save workflows
   |- entity_admin.rs      # resolve/load/delete/undo/ensure helpers
   |- suggestions.rs       # autocomplete and reference suggestions
   `- vault_sync.rs        # startup vault -> db sync
```

`main.rs` is now thin application wiring, not a command business logic sink.

---

## 4. Entity Domain Architecture

Entities now share one additive architecture:

- **Kinds:** `EntityKind` enumerates every supported entity and exposes helpers (`as_str`, `command_root`, `display_name`). All command/service dispatch takes a kind instead of bespoke enums.
- **Schemas:** `EntitySchema` + `EntityFieldSpec` in `entities/schema.rs` declare canonical fields, aliases, value kinds, and access guards. Validation, suggestions, and help text all consume these specs.
- **Domains:** Each entity implements the `EntityDomain` trait (`entities/domain.rs`) to encapsulate help, show, rename, set, reroll, save, and cancel flows. Domain implementations live under `entities/domains/` and use shared helpers from `entities/common.rs`.
- **Registry:** `EntityDomainRegistry` (`entities/registry.rs`) owns `Arc<dyn EntityDomain>` instances. Command handlers resolve domains by kind, so adding a new entity means registering it once during startup.
- **Editor session:** `app_state.rs` stores drafts in a `HashMap<EntityKind, DraftEnvelope>`. `active_kind` drives `system save/reroll/cancel`. New entities only need `set_<kind>/get_<kind>` helpers plus `DraftEnvelope` variants.

This setup keeps command modules small and makes onboarding new entity types a mostly additive change set: define schema, implement domain, register it, wire persistence/reroll, and expose CLI + frontend hooks.

### Wizard Framework

Multi-step *wizards* (guided flows like `create dungeon` that ask a sequence of questions before producing an artifact) use the same additive, registry-backed pattern as entities — deliberately mirrored so the two read the same way. A wizard is **declarative data plus one trait impl**; the plumbing (dispatch, navigation verbs, clickable prompts, autocomplete context, the spinner signal) lives in `wizards/` once and never changes per wizard.

| Entity domain (one-shot create) | Wizard (multi-step flow) |
|---|---|
| `EntityKind` variant | stable `id()` string (`"dungeon"`) |
| `EntitySchema` + `EntityFieldSpec` | ordered `WizardStep`s (declarative) |
| `EntityDomain` trait (`entities/domain.rs`) | `Wizard` trait (`wizards/wizard.rs`) |
| `EntityDomainRegistry` + `build_default_registry()` | `WizardRegistry` + `build_default_wizard_registry()` |
| `EditorSession` (`active_kind` + draft map) | `WizardSession` (`active_id` + cursor + history + type-erased data) |
| `InputContext::EntityEditor(kind)` | `InputContext::Wizard(id)` |
| `CommandAvailability::EntityScoped` | `CommandAvailability::AnyWizard` (`continue`/`back`) + `AnyEditorOrWizard` (`cancel`) |
| Card footer emits `command_ref` (`drafts.rs`) | Step prompt emits `command_ref` **by construction** (`wizards/prompt.rs`) |

Key pieces:

- **`Wizard` + `WizardStep` traits** (`wizards/wizard.rs`): a wizard exposes `id/title/steps/seed/finalize`; each step exposes `prompt/choices/awaiting_llm_label/accept`. `accept()` returns a `WizardTransition` (`Stay`/`Next`/`Goto(id)`/`Back`/`Complete`/`Cancel`) and is — with `finalize()` — the *only* host-coupled surface (it takes `&AppState`); everything else is host-agnostic so the engine can later move to a shared crate.
- **`WizardSession`** (`wizards/session.rs`): the live cursor, a history stack that powers `back`, and a type-erased `WizardData` accumulator (`Box<dyn Any>`, the wizard analogue of `DraftEnvelope`). Lives on `AppState` next to `wizards: Arc<WizardRegistry>`.
- **One dispatch route** (`wizards/runtime.rs`): `try_execute_active_wizard` handles the global nav verbs (`cancel`/`back`), delegates to the active step's `accept()`, applies the transition, and renders the next prompt (or runs `finalize`). It populates the structured `CommandResponse.wizard: WizardView { id, step_id, awaiting_llm_label }` that drives the frontend spinner — no prompt-text matching.
- **Clickability by construction** (`wizards/prompt.rs`): the sanctioned prompt builders (`wizard_menu`, `action_row`, `choice_lines`) render every `WizardChoice` as a `command_ref`, so an author *cannot* emit a non-clickable choice.
- **Autocomplete for free**: `resolve_input_context` returns `InputContext::Wizard(id)` while a wizard is active, so the generic availability filter surfaces the nav verbs; `active_step_choices` feeds the step's tokens into the suggestion service.

The dungeon wizard (`wizards/dungeon.rs`) is the reference implementation. See §8D for the "Add a New Wizard" playbook.

## 5. Command Manifest and Metadata Rules

The manifest in `command-specs/src/lib.rs` is the single source of truth for:

- command names, subcommands, examples
- aliases
- execution target (`Core` or `Desktop`)
- autocomplete visibility (`show_in_autocomplete`)
- subcommand requirement (`requires_subcommand` — drives the parser's argument-vs-subcommand decision)
- canonical help command for clickability

The same file also owns **context availability** via `command_availability(name)` and the `InputContext`/`CommandAvailability` enums. This is the single source of truth for which commands appear in which context (default surface, setup wizard, entity editor), consumed by both autocomplete and the help index. Adding a command without an explicit availability arm leaves it `Default`-only (invisible in editors) — a common regression. The parser's `requires_subcommand` semantics and the help↔autocomplete parity are documented in `docs/command-contexts.md`; read it before changing visibility, parsing, or help.

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
- `VaultSyncService` handles startup scan and reconciliation

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

1. **Schema + kind**: add `EntityKind` variant, schema constants, and helper exports in `entities/{kind,schema}.rs`.
2. **Domain**: implement `<Entity>Domain` under `entities/domains/`, hook into shared helpers, and register it inside `build_default_registry()`.
3. **Draft storage**: add `DraftEnvelope::<Entity>` variants plus `get_/set_/take_` helpers in `app_state.rs`.
4. **Data layer**: create migration (`core/migrations`), extend `core/src/db.rs`, and add repository trait + prod impl.
5. **Services**: add persistence/reroll/admin logic (`services/entity_{persistence,reroll,admin}.rs`) and ensure vault sync covers the new table.
6. **Commands**: add CLI module (pattern after `commands/item_commands.rs`) and register handler entries.
7. **Shared entity commands**: update `commands/entity_commands.rs` load/show/delete/undo flows to hydrate drafts + events; extend `SuggestionService` filters as needed.
8. **Front-end + contracts**: add draft/frontmatter + card builder + events to `runebound-models`, regenerate TS, and handle the client event in `desktop/src/App.tsx`.
9. **Docs/tests**: update `docs/` playbooks and run the verification checklist.

### D) Add a New Wizard (multi-step guided flow)

Mirrors C, but for a sequence of prompts rather than a one-shot create. The plumbing is already written; a new wizard is additive data + one trait impl (see §4 Wizard Framework).

1. **Steps + data**: in a new `wizards/<name>.rs`, define the accumulator struct and one `WizardStep` impl per step. Build prompts only with the `wizards/prompt.rs` helpers (`wizard_menu`/`action_row`/`choice_lines`) so every choice is a clickable `command_ref`. Set `awaiting_llm_label()` on any step whose submission calls the LLM.
2. **Wizard impl**: implement the `Wizard` trait (`id/title/steps/seed/finalize`). `finalize()` builds the artifact and hands off (open an entity draft, write config, …) exactly as a create handler would.
3. **Register**: add one line in `build_default_wizard_registry()` (`wizards/registry.rs`).
4. **Launch**: point the entry command at `start_wizard("<id>", state)` (mirror `create dungeon` in `create_commands.rs`).
5. **No plumbing edits**: dispatch (`try_execute_active_wizard`), the nav verbs, `InputContext::Wizard`, the spinner signal, and step-token autocomplete all work without changes. The nav verbs already have `command_availability` arms (`continue`/`back` = `AnyWizard`, `cancel` = `AnyEditorOrWizard`).
6. **Verify**: `cargo test suggestions` (step-token typeahead), then manually walk the flow — every step's choices clickable, `back`/`cancel` at each step, spinner on LLM steps, `finalize` produces the artifact.

The only edits outside `wizards/<name>.rs` are the one registry line and the launch call. If a wizard needs a new step *capability* (a new `WizardTransition`, a config seed), that is an engine change in `wizards/` shared by all wizards — see `docs/onboarding-wizard-port.md` for the planned extensions.

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

---

## 10. Known Friction Points

The refactor provides a strong base, but these are still active complexity points:

- high duplication across `npc/location/faction` command modules
- large service modules (`entity_admin`, `entity_reroll`, `suggestions`) that may benefit from entity-agnostic abstractions
- many explicit type branches when adding a brand-new entity class

This is acceptable for current velocity, but if entity count grows rapidly, consider shared entity capability traits and more table-driven field specs.

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

*Last updated: 2026-06-15*  
*If this document drifts from the codebase, update it in the same PR as the architecture change.*
