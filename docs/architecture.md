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
| `runebound-models` | Shared models + TS generation | `NpcDraft`, `LocationDraft`, `FactionDraft`, `OutputDoc`, `events` |
| `desktop/src-tauri` | Desktop command backend | `commands/`, `services/`, `repositories/`, `router.rs`, `main.rs` |

---

## 2. Command Dispatch Architecture

All commands follow one path:

1. Parse input
2. Normalize aliases/help form
3. Resolve root token via registry
4. Execute handler
5. Return `CommandResponse`

There are two registries using the same `command-handler` crate:

- Core registry in `core/src/command.rs` for `status`, `config`, `help`, `exit`, `setup`
- Desktop registry in `desktop/src-tauri/src/commands/mod.rs` for desktop interaction commands

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
|- app_state.rs            # AppState, EditorSession, EditorMode
|- commands/
|  |- mod.rs               # registry construction + shared response helpers
|  |- create_commands.rs   # create npc|location|faction
|  |- npc_commands.rs      # npc show|rename|set|travel|reroll|save|cancel
|  |- location_commands.rs # location show|rename|set|reroll|save|cancel
|  |- faction_commands.rs  # faction show|rename|set|reroll|save|cancel
|  |- entity_commands.rs   # load|show|preview|delete|undo
|  `- system_commands.rs   # mode-aware save|reroll|cancel
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

## 4. Command Manifest and Metadata Rules

The manifest in `command-specs/src/lib.rs` is the single source of truth for:

- command names, subcommands, examples
- aliases
- execution target (`Core` or `Desktop`)
- autocomplete visibility
- canonical help command for clickability

Manifest data is consumed by:

- backend suggestion service (`desktop/src-tauri/src/services/suggestions.rs`)
- help renderers in `core/src/command.rs`
- desktop/core registry metadata generation
- frontend command clickability fallbacks

If you rename or add command tokens without updating manifest entries, help/autocomplete/clickability will drift.

---

## 5. Repository and Service Boundaries

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

## 6. Shared Models and Contracts

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

## 7. Extension Playbooks

### A) Add New Top-Level Command

1. Add `CommandSpec` in `command-specs/src/lib.rs`
2. Implement handler in `desktop/src-tauri/src/commands/<domain>_commands.rs` (or `core/src/command.rs` for core)
3. Register handler entry in `desktop/src-tauri/src/commands/mod.rs` (or core registry)
4. Verify help/autocomplete/clickability paths

No router changes are needed for normal top-level command additions.

### B) Add New Subcommand

1. Add subcommand metadata in `command-specs/src/lib.rs`
2. Implement syntax and behavior in the domain command module
3. Update suggestion behavior if field/value completion is expected
4. Add/verify phrase help output

### C) Add New Entity Type (example: `quest`, `item`, `dungeon`)

1. Add row model + CRUD in `core/src/db.rs` and migration SQL
2. Add repository trait + production impl in `desktop/src-tauri/src/repositories/mod.rs`
3. Add draft/frontmatter model and card builder in `runebound-models/src/drafts.rs`
4. Add event variants in `runebound-models/src/events.rs` if needed
5. Extend `AppState` and `EditorMode` in `desktop/src-tauri/src/app_state.rs`
6. Add domain command module and register it in `desktop/src-tauri/src/commands/mod.rs`
7. Extend entity load/show/delete/undo resolution paths in `desktop/src-tauri/src/commands/entity_commands.rs` and `desktop/src-tauri/src/services/entity_admin.rs`
8. Extend persistence/reroll/generation services as required
9. Extend suggestion filtering/completion in `desktop/src-tauri/src/services/suggestions.rs`
10. Update frontend event handling/rendering in `desktop/src/App.tsx`
11. Update vault sync scan/import support in `desktop/src-tauri/src/services/vault_sync.rs`

---

## 8. Anti-Patterns

| Anti-Pattern | Why It Is Wrong | Correct Approach |
|---|---|---|
| Command business logic in `router.rs` | Breaks dispatch-only router contract | Put behavior in `commands/*.rs` and services |
| Direct `core::db` calls from handlers | Bypasses testable boundaries | Use repositories from `AppState` |
| Duplicated cross-layer types | Causes drift between Rust and TS | Use `runebound-models` first |
| Large, ad-hoc parsing in frontend for command semantics | Duplicates backend command rules | Keep parser authority backend-first |
| Depending on markdown heuristics for command links | Fragile clickability | Emit explicit `command_ref` nodes |

---

## 9. Known Friction Points

The refactor provides a strong base, but these are still active complexity points:

- high duplication across `npc/location/faction` command modules
- large service modules (`entity_admin`, `entity_reroll`, `suggestions`) that may benefit from entity-agnostic abstractions
- many explicit type branches when adding a brand-new entity class

This is acceptable for current velocity, but if entity count grows rapidly, consider shared entity capability traits and more table-driven field specs.

---

## 10. Feature Development Checklist

Before merging any feature that changes commands/entities:

- [ ] Manifest updates complete in `command-specs/src/lib.rs`
- [ ] Handler implementation placed in correct command domain module
- [ ] Registry registration updated
- [ ] Repository/service boundaries respected (no direct DB from handlers)
- [ ] `output_doc` and `command_ref` used for actionable output
- [ ] Frontend model usage comes from generated `desktop/src/generated/models.ts`
- [ ] Suggestions updated when command surface or fields changed
- [ ] `make build` passes
- [ ] Primary user flow manually verified (help, run, save/cancel, load/show where applicable)

---

## 11. Related Docs

- `docs/cli.md` for command UX contracts and command implementation checklist
- `docs/render.md` for output rendering rules and card/output extension guidance
- `docs/feature-development.md` for end-to-end implementation playbooks

---

*Last updated: 2026-06-14*  
*If this document drifts from the codebase, update it in the same PR as the architecture change.*
