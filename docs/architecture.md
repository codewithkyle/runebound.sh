# Architecture and Design Patterns

> **Purpose:** This document preserves the architectural decisions, patterns, and extension guidelines established during the 2026 refactoring. Future agents MUST read this before modifying command routing, entity types, or the module structure.

---

## 1. Workspace Overview

The project is a Rust workspace with a Tauri desktop frontend. The architecture is split into **crates** by responsibility, with the desktop backend layered internally into **commands**, **repositories**, **services**, and **models**.

### Crates

| Crate | Responsibility | Key Exports |
|---|---|---|
| `core` (`dnd_core`) | Config, database, vault, shared NPC/location logic, command parsing, command execution (TUI + desktop) | `db`, `vault`, `command_manifest`, `command_parse`, `command` |
| `command-handler` | Generic `CommandHandler` trait, `HandlerRegistry`, `HandlerEntry`, `HandlerMetadata` | Reusable dispatch machinery for any frontend |
| `command-specs` | **Single source of truth** for command definitions (manifest, specs, aliases, execution targets) | `command_manifest()`, `CommandManifest`, `CommandSpec`, `HandlerMetadataDescriptor` |
| `runebound-models` | Shared Rust models with **TypeScript codegen** via `build.rs` | `NpcDraft`, `LocationDraft`, `FactionDraft`, `OutputDoc`, `events` |
| `desktop/src-tauri` | Tauri backend; delegates to `core` for non-desktop commands | `commands/`, `repositories/`, `services/`, `router.rs` |

---

## 2. Command Dispatch Architecture

### Philosophy

**All commands flow through the same pattern:**

1. Parse → 2. Normalize → 3. Lookup in registry → 4. Execute handler → 5. Return `CommandResponse`

There are **two registries** that share the same `command-handler` crate:

- **`core` registry** (`core/src/command.rs`): For `Core` execution commands (e.g., `status`, `config`, `help`, `exit`, `setup`)
- **Desktop registry** (`desktop/src-tauri/src/commands/mod.rs`): For `Desktop` execution commands (e.g., `create`, `npc`, `location`, `faction`, `load`, `show`, `delete`, `undo`, `save`, `reroll`, `cancel`, `clear`, `history`)

### The Three Types

```text
CommandSpec (in command-specs)
    ↓
HandlerMetadata (in command-handler)
    ↓
HandlerEntry<Bridge> (in command-handler)
    ↓
HandlerRegistry<Bridge> (in command-handler)
```

- **`CommandSpec`** — Declarative metadata (name, summary, subcommands, examples, execution target). This is the **single source of truth**.
- **`HandlerMetadata`** — Runtime metadata for the registry (summary, examples, aliases, execution target). Converted from `CommandSpec` via `handler_metadata_for()`.
- **`HandlerEntry`** — A named handler with metadata and a `HandlerBridge` implementation.
- **`HandlerRegistry`** — A `HashMap<&'static str, HandlerEntry>` that resolves the first token of a command to its handler.

### The Bridge Pattern

Both core and desktop use a **bridge** to adapt their specific invocation context to the generic `CommandHandler` trait:

```rust
// Core
struct CoreHandler {
    inner: Arc<dyn Fn(CoreHandlerInvocation<'_>) -> CoreHandlerFuture<'_> + Send + Sync>,
}

// Desktop
struct DesktopHandler {
    inner: Arc<dyn Fn(DesktopHandlerInvocation<'_>) -> CommandHandlerFuture<'_> + Send + Sync>,
}
```

Each has a different `Invocation<'a>` struct because the desktop needs `State<'_, AppState>` while the core needs `&Path` (workspace root) and `&mut SessionState`.

### The Registry Builder

```rust
fn build_desktop_handler_registry() -> HandlerRegistry<DesktopHandler> {
    let mut registry = HandlerRegistry::new();
    registry.register(exit_handler_entry());
    registry.register(clear_handler_entry());
    // ... one entry per top-level command
    registry
}
```

**Adding a new top-level command requires exactly one new `register()` call here.**

---

## 3. Command Manifest & Specs

### The Golden Rule

> **The command manifest is the single source of truth.** Every command, subcommand, alias, example, and execution target lives in `command-specs/src/lib.rs`.

The manifest serves:
- **Autocomplete** (`suggest_command_input` in `main.rs` reads it)
- **Help text generation** (`render_command_help`, `render_subcommand_help`)
- **Registry metadata** (`handler_metadata_for()`)
- **Execution target routing** (`Core` vs `Desktop`)

### Adding a New Top-Level Command

1. Open `command-specs/src/lib.rs`
2. Add a `CommandSpec` to the `commands` vec in `command_manifest()`:
   ```rust
   CommandSpec {
       name: "quest".to_string(),
       summary: "Create and manage quests".to_string(),
       examples: vec!["quest show".to_string()],
       subcommands: vec![
           SubcommandSpec { name: "show".to_string(), summary: "...".to_string(), ... },
       ],
       options: Vec::new(),
       requires_subcommand: true,
       canonical_help_command: Some("quest help".to_string()),
       execution: CommandExecution::Desktop,
       show_in_autocomplete: true,
   }
   ```
3. Add any aliases to the `aliases` vec.

---

## 4. Desktop Layer Architecture

### Module Layout

```
desktop/src-tauri/src/
├── main.rs              # Tauri commands, suggestion engine, vault sync, startup
│                        # ⚠️ Still contains legacy business logic (see §9)
├── router.rs            # 51 lines. Dispatches to desktop registry or core service.
├── app_state.rs         # AppState, EditorSession, EditorMode, draft type aliases
├── commands/
│   ├── mod.rs           # DesktopHandlerRegistry, ok_response helpers, all handler entries
│   ├── create_commands.rs   # create npc|location|faction
│   ├── npc_commands.rs      # npc show|rename|set|travel|reroll|save|cancel
│   ├── location_commands.rs # location show|rename|set|reroll|save|cancel
│   ├── faction_commands.rs  # faction show|rename|set|reroll|save|cancel
│   ├── entity_commands.rs   # load, show, preview, delete, undo
│   └── system_commands.rs   # save, reroll, cancel (mode-aware)
├── repositories/
│   └── mod.rs           # Repository traits + Prod* implementations
├── services/
│   ├── mod.rs           # Module declarations
│   └── ai_generation.rs # AiGenerationService (generate_*_seed, prompt context, schema)
└── utils.rs             # ⚠️ Transitional bridge types (see §9)
```

### Design Principle: One File Per Command Domain

Each command domain (create, npc, location, faction, entity, system) has its own file. This is **not** optional. Do not add new command logic to `router.rs` or `main.rs`.

---

## 5. Repository Pattern

### Philosophy

All database and vault access goes through **traits** with **production implementations**. This enables:
- Unit testing with mock repositories
- Consistent error handling (`Result<T, String>`)
- Clear boundaries between business logic and I/O

### Current Repositories

| Trait | Prod Impl | Wraps |
|---|---|---|
| `NpcRepository` | `ProdNpcRepository` | `core::db` NPC queries |
| `LocationRepository` | `ProdLocationRepository` | `core::db` location queries |
| `FactionRepository` | `ProdFactionRepository` | `core::db` faction queries |
| `DocumentRepository` | `ProdDocumentRepository` | `core::db` document index queries |
| `GenerationRepository` | `ProdGenerationRepository` | `core::db` generation history |
| `SoftDeleteRepository` | `ProdSoftDeleteRepository` | `core::db` soft-delete table |
| `VaultRepository` | `ProdVaultRepository` | `Vault` read/write/move/exist checks |

### Accessing Repositories

```rust
let database = state.database();
let npc_repo = state.npc_repo();
npc_repo.find_by_name_or_slug(database.as_ref(), "Elara").await?;
```

**Rule:** Command handlers never call `core::db` functions directly. Always go through the repository trait.

---

## 6. Service Layer

### AiGenerationService

The `AiGenerationService` struct encapsulates all Ollama API calls:

```rust
impl AiGenerationService {
    pub async fn generate_npc_seed(...) -> Result<NpcSeed, String>
    pub async fn generate_location_seed(...) -> Result<LocationSeed, String>
    pub async fn generate_faction_seed(...) -> Result<FactionSeed, String>
}
```

**Rule:** Command handlers (`create_commands.rs`, `system_commands.rs`) call the service. They do not build HTTP clients or JSON schemas directly.

---

## 7. Shared Models (runebound-models)

### Philosophy

Rust and TypeScript must share the same types. We use `runebound-models` with a `build.rs` script that generates `desktop/src/generated/models.ts`.

### Key Types

- `NpcDraft`, `LocationDraft`, `FactionDraft` — Editor session state
- `OutputDoc`, `OutputBlock`, `InlineNode` — Structured command output
- `CommandClientEvent`, `CommandResponse`, `OutputSegment` — Event/response types
- `events` module — Draft-specific event builders (e.g. `npc_entity_card()`)

### Rule: Add new entity types here first

1. Add Rust structs to `runebound-models/src/`
2. Add to `build.rs` TypeScript generation
3. Import in frontend via `desktop/src/generated/models.ts`
4. Never define a separate TS interface for the same concept.

---

## 8. Extending the System

### Adding a New Top-Level Command

**Example:** Adding a `quest` command.

1. **`command-specs/src/lib.rs`**
   - Add `CommandSpec` for `quest` with subcommands and `execution: Desktop`

2. **`desktop/src-tauri/src/commands/quest_commands.rs`** (new file)
   - Implement `pub async fn handle_quest(invocation: DesktopHandlerInvocation<'_>) -> Result<Option<CommandResponse>, String>`
   - Handle subcommands: `show`, `set`, `rename`, `reroll`, `save`, `cancel`, `help`
   - Use `ok_response()` and `CommandClientEvent` for UI events

3. **`desktop/src-tauri/src/commands/mod.rs`**
   - Add `pub mod quest_commands;`
   - Add `quest_handler_entry()` function
   - Register it in `build_desktop_handler_registry()`

4. **`desktop/src-tauri/src/router.rs`**
   - **No changes needed.** The registry already dispatches by first token.

### Adding a New Entity Type

**Example:** Adding a `Quest` entity type.

1. **`core/src/db.rs`**
   - Add `QuestRow` struct
   - Add SQL queries: `find_quest_by_name_or_slug`, `upsert_quest`, `search_quests_by_name`, `delete_quest_by_id`, `list_quests`

2. **`desktop/src-tauri/src/repositories/mod.rs`**
   - Add `QuestRepository` trait
   - Add `ProdQuestRepository`

3. **`runebound-models`**
   - Add `QuestDraft` struct
   - Add `events::quest_entity_card()` builder
   - Add to `build.rs` TypeScript generation

4. **`desktop/src-tauri/src/app_state.rs`**
   - Add `quest_repo: Arc<dyn QuestRepository>` to `AppState`
   - Add `quest_draft: Option<QuestDraft>` to `EditorSession`
   - Add `EditorMode::Quest` variant

5. **`desktop/src-tauri/src/commands/create_commands.rs`**
   - Add `create quest` handling
   - Call `AiGenerationService::generate_quest_seed(...)` (add to service first)

6. **`desktop/src-tauri/src/commands/quest_commands.rs`** (new file)
   - Implement `show`, `rename`, `set`, `reroll`, `save`, `cancel`, `help`

7. **`desktop/src-tauri/src/commands/entity_commands.rs`**
   - Add `EntityType::Quest` variant
   - Update `build_load_response`, `build_preview_response`, `build_entity_card_doc`, `build_entity_card_text`

8. **`desktop/src-tauri/src/commands/mod.rs`**
   - Register `quest` handler entry

9. **`desktop/src-tauri/src/main.rs`**
   - Add `scan_quest_row_from_markdown()`
   - Update `sync_database_from_vault()` to scan `quests/` directory
   - Update `suggest_command_input()` to show/hide `quest` suggestions based on `EditorMode::Quest`

10. **`desktop/src/App.tsx`** (frontend)
    - Add `QuestDraft` card rendering

### Adding a New Subcommand to an Existing Command

1. Add the subcommand to `CommandSpec` in `command-specs/src/lib.rs`
2. Add the match arm in the relevant `commands/*.rs` handler
3. **Do not** touch the router or registry unless it's a new top-level command.

---

## 9. Current Transitional State

### What Is Clean

- ✅ `command-handler` crate — generic, reusable, no business logic
- ✅ `command-specs` crate — single source of truth for command metadata
- ✅ `router.rs` — 51 lines, pure dispatch
- ✅ `commands/` modules — one domain per file, no cross-domain leakage
- ✅ `repositories/` — trait-based, consistent
- ✅ `services/ai_generation.rs` — clean service boundary
- ✅ `runebound-models` — shared Rust + TS types

### What Is Still Legacy

- ⚠️ **`main.rs` is 5,575 lines** because it still contains the original implementations of:
  - `generate_npc_seed`, `generate_location_seed`, `generate_faction_seed`
  - `reroll_npc_field`, `reroll_location_field`, `reroll_faction_field`
  - `save_npc_draft`, `save_location_draft`, `save_faction_draft`
  - `sync_database_from_vault`
  - `suggest_command_input` (autocomplete engine)
  - Markdown scanning: `scan_npc_row_from_markdown`, `scan_location_row_from_markdown`, `scan_faction_row_from_markdown`

  These are **business logic implementations**, not plumbing. The next refactoring phase should move them into `services/` or `repositories/` so `main.rs` becomes pure Tauri command wiring.

- ⚠️ **`utils.rs` is 1,134 lines of bridge code** — It exists because `main.rs` and `commands/` use different type namespaces. `utils.rs` converts `crate::NpcRerollContext` ↔ `utils::NpcRerollContext`, etc. Once the legacy implementations in `main.rs` are moved into `services/` and `commands/` uses the same types directly, `utils.rs` can shrink dramatically.

- ⚠️ **AI generation logic exists in two places** — `services/ai_generation.rs` has the clean service methods that `commands/` calls. `main.rs` still has the original free functions that `utils.rs` wraps. The `services/` version is the canonical path. The `main.rs` versions are legacy and should be removed in the next phase.

### What This Means for Agents

- **Do not add new logic to `main.rs`.** Put it in the appropriate `commands/` module or `services/` file.
- **Do not add new command dispatch logic to `router.rs`.** Use the registry.
- **Do not duplicate types in `utils.rs`.** If you need a type, use the one from `runebound-models` or `services::ai_generation` directly.
- **When calling a function that exists in both `main.rs` and `services/`, prefer the `services/` version.**

---

## 10. Anti-Patterns (Do Not Do These)

| Anti-Pattern | Why It's Wrong | Correct Approach |
|---|---|---|
| Inline command logic in `router.rs` | `router.rs` is dispatch-only; adding logic here breaks the architecture | Create a `commands/*.rs` module and register it |
| Bypass repository traits to call `core::db` directly | Breaks testability, couples commands to SQL schema | Add a repository trait and use `state.npc_repo()` |
| Define frontend types independently of `runebound-models` | Creates drift between Rust and TS | Add to `runebound-models` and generate TS |
| Add new entity logic without updating `entity_commands.rs` | Load/show/preview/delete will be broken | Always add to `EntityType`, `EntityDetails`, and `build_*` helpers |
| Duplicate `AiGenerationService` logic in a command handler | Service is the canonical boundary | Add a method to `AiGenerationService` and call it |
| Use `crate::SomeType` in command handlers when `utils::SomeType` exists | Deepens the bridge-code mess | Import from `runebound_models` or `services::ai_generation` directly |
| Add `if lowered == "..."` chains in `main.rs` | Reverts to the pre-refactor architecture | Use the registry + `commands/*.rs` modules |

---

## 11. Future Refactoring Targets

These are known, intentional next steps. They are **not** bugs or oversights.

1. **Move `save_*_draft`, `reroll_*_field`, `generate_*_seed` from `main.rs` into `services/`**
   - Estimated reduction: ~2,000 lines from `main.rs`
   - Then remove `utils.rs` bridge code

2. **Move `scan_*_row_from_markdown` and `sync_database_from_vault` into `repositories/` or a new `services/vault_sync.rs`**
   - Estimated reduction: ~800 lines from `main.rs`

3. **Move `suggest_command_input` autocomplete engine into `commands/` or a new `services/suggestions.rs`**
   - Estimated reduction: ~600 lines from `main.rs`

4. **Move `main.rs` free functions (normalization, validation, path helpers) into `core` or `services/` as appropriate**
   - Target: `main.rs` under 500 lines (pure Tauri command wiring)

5. **Add `async-trait` to `command-handler` if we want to avoid the `HandlerBridge` closure pattern**
   - Currently the bridge uses `Arc<dyn Fn(...)>` for lifetime flexibility. This is idiomatic but verbose.

---

## 12. Quick Reference

### Where to add code for common tasks

| Task | File(s) |
|---|---|
| New top-level command | `command-specs/src/lib.rs`, `commands/<domain>_commands.rs`, `commands/mod.rs` |
| New subcommand | `command-specs/src/lib.rs` (spec), `commands/<domain>_commands.rs` (impl) |
| New entity type | `runebound-models`, `core/src/db.rs`, `repositories/mod.rs`, `app_state.rs`, `commands/entity_commands.rs`, `commands/create_commands.rs`, `services/ai_generation.rs` |
| New AI generation flow | `services/ai_generation.rs` |
| New database query | `core/src/db.rs`, then `repositories/mod.rs` trait |
| New frontend type | `runebound-models/src/` + `build.rs` |
| New autocomplete behavior | `main.rs` (legacy, move to `services/` when possible) |
| New vault sync behavior | `main.rs` (legacy, move to `services/` when possible) |

### Type Import Hierarchy

```text
runebound-models           ← canonical source of truth
    ↓
desktop commands/          ← import from runebound-models
    ↓
desktop services/          ← import from runebound-models and commands/
    ↓
desktop repositories/      ← import from core::db, re-export as traits
    ↓
core                       ← owns db, vault, config, parse
```

**Never import from `utils.rs` into new code.** `utils.rs` is a transitional bridge. Import from `runebound_models` or `services::ai_generation` directly.

---

*Last updated: 2026-06-14*  
*If this document is outdated, update it before adding new features.*
