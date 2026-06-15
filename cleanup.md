# Main.rs Cleanup Plan

> **Purpose:** This document is the final cleanup roadmap for `desktop/src-tauri/src/main.rs`. After the 2026 refactoring, `main.rs` still contains ~5,500 lines of legacy business logic that predates the `commands/`/`repositories/`/`services/` architecture. This document splits the extraction into **phases** with clear deliverables, dependencies, and context so any agent can implement it.

---

## 0. Current State (Why This Exists)

After the 2026 refactoring:

- ✅ `router.rs` is 51 lines — pure dispatch, zero business logic
- ✅ `commands/` modules — one domain per file, all command handlers
- ✅ `repositories/` — trait-based DB/vault access
- ✅ `services/ai_generation.rs` — clean `AiGenerationService` for Ollama calls
- ✅ `utils.rs` — bridge types with `From` impls between type namespaces
- ❌ `main.rs` is **5,575 lines** because it still contains the original implementations of:
  - AI generation (`generate_*_seed`, `reroll_*_field`)
  - Entity persistence (`save_*_draft`)
  - Vault sync (`sync_database_from_vault`, `scan_*_row_from_markdown`)
  - Autocomplete engine (`suggest_command_input`, `build_command_suggestions`)
  - Vault reference system (`load_vault_reference_entries`, `build_reference_suggestions`)
  - Normalization/validation helpers (`normalize_*`, `validate_*`, `canonical_*`)
  - ~30 struct definitions that duplicate `utils.rs` and `services/ai_generation.rs` types

### The `utils.rs` Bridge Problem

`utils.rs` contains `From` impls and wrapper functions that bridge between `utils::*` types and `crate::*` types (the ones in `main.rs`). For example:

```rust
// utils.rs
pub async fn reroll_npc_field(input: RerollNpcFieldInput, state: ...) -> Result<RerollNpcFieldResult, String> {
    let internal_input: crate::RerollNpcFieldInput = input.into();
    let result = crate::reroll_npc_field(internal_input, state).await?;
    Ok(result.into())
}
```

This means `utils.rs` cannot be removed until `main.rs` stops being the source of truth for business logic. The canonical implementations must move into `services/` first, then `utils.rs` wrappers can point to `services/` instead of `crate::`, and eventually `utils.rs` can be eliminated.

---

## 1. Phase Overview

| Phase | Scope | Estimated Lines Removed from `main.rs` | Target File(s) |
|---|---|---|---|
| **Phase 1** | Remove duplicate AI generation structs/functions | ~800 | `services/ai_generation.rs` (already canonical) |
| **Phase 2** | Move reroll logic into a service | ~600 | `services/entity_reroll.rs` |
| **Phase 3** | Move save/persistence logic into a service | ~700 | `services/entity_persistence.rs` |
| **Phase 4** | Move vault sync & markdown scanning into a service | ~800 | `services/vault_sync.rs` |
| **Phase 5** | Move autocomplete engine into a service | ~600 | `services/suggestions.rs` |
| **Phase 6** | Move normalization helpers into `utils/` or `core` | ~700 | `utils.rs`, `core` |
| **Phase 7** | Eliminate `utils.rs` bridge code and consolidate types | ~400 | `utils.rs` (deleted/reduced) |
| **Phase 8** | Final cleanup: `main.rs` becomes pure Tauri wiring | ~300 | `main.rs` |

**Target:** `main.rs` under **500 lines** (imports, `main()`, and 3-4 Tauri command wrappers).

---

## Phase 1: Remove Duplicate AI Generation Code *(✅ Completed 2026-06-14)*

### What's in `main.rs`

Lines ~2600-3900 contain `generate_npc_seed`, `reroll_npc_field`, `generate_location_seed`, `reroll_location_field`, `generate_faction_seed`, `reroll_faction_field` — the original implementations.

### What's already in `services/ai_generation.rs`

The `AiGenerationService` struct already has clean `generate_npc_seed`, `generate_location_seed`, `generate_faction_seed` methods. The `commands/` modules (`create_commands.rs`, `system_commands.rs`) already call these service methods.

**But:** `reroll_*_field` is still in `main.rs`, and `utils.rs` calls `crate::reroll_npc_field` (which is in `main.rs`).

### Deliverables

1. **Add `reroll_*_field` methods to `AiGenerationService`**
   - Move `reroll_npc_field` logic from `main.rs` into `AiGenerationService::reroll_npc_field()`
   - Move `reroll_location_field` logic into `AiGenerationService::reroll_location_field()`
   - Move `reroll_faction_field` logic into `AiGenerationService::reroll_faction_field()`
   - These methods should take the same inputs as the current free functions but return `Result<RerollNpcFieldResult, String>` etc.

2. **Update `utils.rs` wrappers**
   - Change `utils::reroll_npc_field` to call `services::ai_generation::AiGenerationService::reroll_npc_field` instead of `crate::reroll_npc_field`
   - Same for location and faction

3. **Remove the free functions from `main.rs`**
   - Delete `generate_npc_seed`, `generate_location_seed`, `generate_faction_seed`
   - Delete `reroll_npc_field`, `reroll_location_field`, `reroll_faction_field`
   - Delete `NpcRerollContext`, `LocationRerollContext`, `FactionRerollContext` structs (if not used elsewhere in `main.rs`)
   - Delete `RerollNpcFieldInput`, `RerollLocationFieldInput`, `RerollFactionFieldInput`
   - Delete `RerollNpcFieldResult`, `RerollLocationFieldResult`, `RerollFactionFieldResult`
   - Delete `GenerateNpcSeedInput`, `GenerateLocationSeedInput`, `GenerateFactionSeedInput`
   - Delete `NpcSeed`, `LocationSeed`, `FactionSeed` (these exist in `services/ai_generation.rs` already)
   - Delete `canonical_npc_reroll_field`, `canonical_location_reroll_field`, `canonical_faction_reroll_field` (move to `services/` or `utils/`)
   - Delete `npc_context_summary`, `location_context_summary`, `faction_context_summary` (move to `services/`)

4. **Update imports in `main.rs`**
   - Remove imports that were only used by the deleted functions

5. **Verify**
   - `cargo check` passes
   - `commands/create_commands.rs` still compiles (uses `AiGenerationService`)
   - `commands/system_commands.rs` still compiles (uses `AiGenerationService`)
   - `utils.rs` still compiles (uses `services::ai_generation`)

### Dependencies

None. This is the first phase. The service already exists and is already used.

### Tests

- Run `cargo check -p desktop` (or the desktop package name)
- Run `cargo build` in the desktop directory

---

## Phase 2: Move Reroll Logic into `services/entity_reroll.rs` *(✅ Completed 2026-06-14)*

### Alternative: Extend `services/ai_generation.rs`

If `reroll_*_field` is tightly coupled with `generate_*_seed` (shared HTTP client, shared prompt building, shared schema construction), keep it in `services/ai_generation.rs`. The phase above already adds it there.

**If** the reroll logic is decoupled enough to stand alone, create `services/entity_reroll.rs` with `EntityRerollService`.

### Deliverables

Same as Phase 1, item 1. If already done in Phase 1, skip this phase.

---

## Phase 3: Move Save/Persistence Logic into `services/entity_persistence.rs` *(✅ Completed 2026-06-14)*

### What's in `main.rs`

Lines ~3900-5200 contain `save_npc_draft`, `save_location_draft`, `save_faction_draft`.

These functions:
- Read the config from `workspace_root`
- Validate the vault path
- Generate markdown via `render_npc_markdown` (from `core`)
- Write to vault via `Vault::write_relative`
- Upsert to database via `db::upsert_npc` (from `core`)
- Return `Save*DraftResult` with id, slug, vault_path, timestamps

### What's already in `utils.rs`

`utils.rs` has `SaveNpcDraftInput`, `SaveNpcDraftResult`, `save_npc_draft_impl` which wraps `crate::save_npc_draft`.

### Deliverables

1. **Create `services/entity_persistence.rs`**
   - Create `EntityPersistenceService` struct (or free functions if stateless)
   - Move `save_npc_draft` logic into `EntityPersistenceService::save_npc()` or `save_npc_draft()`
   - Move `save_location_draft` logic into `save_location_draft()`
   - Move `save_faction_draft` logic into `save_faction_draft()`
   - These functions should take `AppState` (or individual repositories) and the draft input
   - Return `Result<SaveNpcDraftResult, String>` etc.
   - Use `state.npc_repo()`, `state.vault_repo()`, `state.document_repo()` for DB/vault access

2. **Update `utils.rs` wrappers**
   - Change `utils::save_npc_draft_impl` to call `services::entity_persistence::save_npc_draft` instead of `crate::save_npc_draft`
   - Same for location and faction

3. **Remove from `main.rs`**
   - Delete `save_npc_draft`, `save_location_draft`, `save_faction_draft`
   - Delete `SaveNpcDraftInput`, `SaveLocationDraftInput`, `SaveFactionDraftInput` (keep in `utils.rs` or `services/`)
   - Delete `SaveNpcDraftResult`, `SaveLocationDraftResult`, `SaveFactionDraftResult` (keep in `utils.rs` or `services/`)
   - Delete `NpcDeletePayload`, `LocationDeletePayload`, `FactionDeletePayload` if they exist here

4. **Verify**
   - `commands/npc_commands.rs` still compiles (uses `utils::save_npc_draft_impl`)
   - `commands/location_commands.rs` still compiles
   - `commands/faction_commands.rs` still compiles
   - `commands/system_commands.rs` still compiles

### Dependencies

- Phase 1 (or Phase 2) must be complete so the remaining `main.rs` code is easier to reason about.
- `repositories/` must already exist (it does).

---

## Phase 4: Move Vault Sync & Markdown Scanning into `services/vault_sync.rs` *(✅ Completed 2026-06-14)*

### What's in `main.rs`

Lines ~2000-2600 contain:
- `collect_markdown_files_under`
- `scan_npc_row_from_markdown`
- `scan_location_row_from_markdown`
- `scan_faction_row_from_markdown`
- `sync_database_from_vault`
- `stable_id_from_relative`
- `file_stem_name`
- `extract_runebound_toml`
- `normalize_relative_path_for_storage`
- `path_for_display`
- `read_vault_file_if_exists`
- `unique_trash_path`
- `move_vault_file`
- `unique_markdown_path_for_name`

These functions deal with:
- Reading the vault filesystem
- Parsing markdown frontmatter (`runebound` TOML blocks)
- Converting markdown files to DB rows
- Syncing the database with the vault on startup

### What's already in `repositories/` and `services/`

- `repositories/mod.rs` has `VaultRepository` trait with `read_file`, `write_file`, `move_file`, `file_exists`, `resolve_path`, `ensure_root_exists`, `ensure_structure`
- `services/` does not yet have vault sync logic

### Deliverables

1. **Create `services/vault_sync.rs`**
   - Create `VaultSyncService` struct
   - Move `sync_database_from_vault` into `VaultSyncService::sync_from_vault()`
   - Move `scan_*_row_from_markdown` into private methods or free functions in `vault_sync.rs`
   - Move `collect_markdown_files_under` into `vault_sync.rs`
   - Move `extract_runebound_toml` into `vault_sync.rs` (or `utils/` if also used elsewhere)
   - Move `stable_id_from_relative`, `file_stem_name` into `vault_sync.rs` or `utils/`
   - Move `normalize_relative_path_for_storage`, `path_for_display` into `utils/` (they're already used by `commands/` and `utils/`)
   - Move `read_vault_file_if_exists`, `unique_trash_path`, `move_vault_file`, `unique_markdown_path_for_name` into `VaultSyncService` or `repositories/` (if they belong with vault operations)
   - Use `state.vault_repo()` and `state.npc_repo()` / `state.location_repo()` / `state.faction_repo()` / `state.document_repo()` for all I/O

2. **Update `main.rs`**
   - Call `VaultSyncService::sync_from_vault()` from `main()` instead of calling the free function directly
   - Delete all the moved functions and structs

3. **Verify**
   - `main.rs` still compiles
   - Startup sync still works (vault files are scanned and DB is updated)

### Dependencies

- Phase 3 should be complete.

### Notes

- `normalize_relative_path_for_storage` and `path_for_display` are used by `commands/entity_commands.rs` and `utils.rs`. Move them to `utils.rs` so all modules can import them.
- `unique_markdown_path_for_name` is used by `save_*_draft` (which will be in `services/entity_persistence.rs` after Phase 3). Move it to `utils/` or `services/vault_sync.rs` and import it from `entity_persistence.rs`.

---

## Phase 5: Move Autocomplete Engine into `services/suggestions.rs` *(✅ Completed 2026-06-14)*

### What's in `main.rs`

Lines ~1450-1900 contain the autocomplete engine:
- `suggest_command_input` (the Tauri command)
- `build_command_suggestions`
- `build_root_suggestions`
- `build_subcommand_suggestions`
- `build_argument_suggestions`
- `find_command`
- `replace_current_token`
- `completion_suffix`
- `starts_with_known_command_root`
- `npc_travel_location_query`

These functions:
- Parse user input
- Read the manifest
- Suggest commands, subcommands, options, entities, locations
- Return `Vec<CommandSuggestion>`

### Deliverables

1. **Create `services/suggestions.rs`**
   - Create `SuggestionService` struct (or free functions)
   - Move `build_command_suggestions`, `build_root_suggestions`, `build_subcommand_suggestions`, `build_argument_suggestions` into `suggestions.rs`
   - Move `find_command`, `replace_current_token`, `completion_suffix` into `suggestions.rs`
   - Move `starts_with_known_command_root` into `suggestions.rs` or `utils/`
   - Move `npc_travel_location_query` into `suggestions.rs` (it's autocomplete-specific)
   - The `suggest_command_input` Tauri command in `main.rs` becomes a thin wrapper:
     ```rust
     #[tauri::command]
     async fn suggest_command_input(input: String, state: State<'_, AppState>) -> Result<Vec<CommandSuggestion>, String> {
         services::suggestions::build_suggestions(input, state).await
     }
     ```

2. **Update `main.rs`**
   - Keep the `#[tauri::command]` wrapper but delegate to `services::suggestions`
   - Delete the moved functions

3. **Verify**
   - Autocomplete still works for commands, subcommands, entities, and locations
   - `Tab` completion still works

### Dependencies

- Phase 4 should be complete.

### Notes

- `suggest_command_input` uses `state` to access `editor_session` for `EditorMode` filtering. The service will need access to the mode, or the filtering logic can stay in the Tauri command wrapper.
- `suggest_command_input` also calls `search_entities` and `search_location_names` — these are DB queries. The service can use `state.npc_repo().search_by_name()` etc.

---

## Phase 6: Move Normalization Helpers into `utils/` or `core` *(✅ Completed 2026-06-14)*

### What's in `main.rs`

Lines ~500-760 and ~875-1000 contain:
- `normalize_sex`
- `normalize_unknown_text`
- `normalize_unknown_list`
- `parse_carrying_csv`
- `normalize_location_kind_type`
- `normalize_location_danger_level`
- `parse_list_csv`
- `normalize_exports`
- `normalize_location_seed`
- `validate_location_details`
- `normalize_faction_kind_type`
- `normalize_faction_seed`
- `validate_faction_details`
- `carrying_to_db_text`
- `carrying_from_db_text`
- `faction_list_to_db_text`
- `faction_list_from_db_text`
- `sentence_count`
- `word_count`
- `validate_sentence_range`

These are pure utility functions. Some are duplicated in `utils.rs`.

### Deliverables

- All normalization helpers (`normalize_*`, `sentence_count`, `validate_*`) now live in `utils.rs` and are reused by services/tests; the duplicate implementations in `main.rs` and `services/ai_generation.rs` have been deleted.
- Canonical DB serialization helpers (`carrying_*`, `exports_*`, `faction_list_*`) moved into `core/src/serialization.rs` and are imported where needed (`services/entity_persistence.rs`, `main.rs`).
- `services/ai_generation.rs` imports the shared helpers instead of redefining them, shrinking `main.rs` by another ~70 lines.
- Normalization tests relocated from `main.rs` into `utils.rs`, and new serialization unit tests were added under `dnd-core`.

### Dependencies

- Phases 1-5 should be complete.

### Notes

- Many of these are already duplicated in `utils.rs`. The `utils.rs` versions are the ones used by `commands/`. The `main.rs` versions are the legacy ones used by the legacy functions in `main.rs`. Once the legacy functions are moved out, the `main.rs` duplicates can be deleted.

---

## Phase 7: Eliminate `utils.rs` Bridge Code *(✅ Completed 2026-06-14)*

### The Problem

`utils.rs` exists because `main.rs` and `commands/` used different type namespaces. Example:

```rust
// main.rs
struct NpcRerollContext { ... }

// utils.rs
struct NpcRerollContext { ... }
impl From<crate::NpcRerollContext> for utils::NpcRerollContext { ... }
impl From<utils::NpcRerollContext> for crate::NpcRerollContext { ... }
```

After Phases 1-6, the canonical types should all live in `services/` or `runebound-models`, and `commands/` should import them directly. `utils.rs` should shrink to only shared helper functions (normalization, path helpers).

### Deliverables

- Added a dedicated `services/entity_admin.rs` module that owns `EnsureLocation*`, `EntityDetails/EntityType`, and the soft-delete/undo logic. All consumers (commands, router, suggestions) now call this service directly.
- Updated every command module to import the canonical service types (`entity_reroll`, `entity_persistence`, `entity_admin`) instead of the `utils.rs` wrappers.
- Removed the async forwarding functions and duplicate struct definitions from `utils.rs`; it now contains only shared normalization/path helpers that are reused by services/tests.
- Deleted the legacy implementations from `main.rs` so it no longer exposes business logic; it simply wires the Tauri commands and startup services.
- Ran `cargo fmt`, `cargo check` (desktop), and `cargo test -p dnd-core` to verify the refactor.

### Dependencies

- All previous phases must be complete.

### Target

`utils.rs` should be under **300 lines** (from ~1,100). It should contain only:
- Normalization helpers
- Path helpers
- Reroll canonicalization helpers
- Context summary builders
- No `From` impls bridging `crate::*` to `utils::*`

---

## Phase 8: Final `main.rs` Cleanup *(✅ Completed 2026-06-14)*

### Target State

`main.rs` should be under **500 lines** and contain only:

1. **Module declarations** (`mod app_state; mod commands; mod repositories; mod router; mod services; mod utils;`)
2. **Imports** (Tauri, serde, tokio, dnd_core, crate modules)
3. **Tauri command wrappers** (`run_command`, `suggest_command_input`, `get_command_manifest`, `exit_app`)
4. **`main()`** — `tauri::Builder`, `.invoke_handler()`, `.run()`
5. **Tests** (if any remain that test Tauri wiring)

### What's Removed

- All struct definitions (moved to `services/` or `utils/` or `runebound-models`)
- All business logic (moved to `services/`)
- All vault sync logic (moved to `services/vault_sync.rs`)
- All autocomplete logic (moved to `services/suggestions.rs`)
- All normalization helpers (moved to `utils/` or `core`)
- All reference/vault helpers (moved to `services/vault_sync.rs` or `utils/`)

Result: `main.rs` is down to **139 lines**, containing only module declarations, imports, the four Tauri command wrappers, and `main()`.

### Deliverables

1. **Audit `main.rs` for remaining dead code**
   - Check for unused imports
   - Check for unused structs/functions
   - Delete anything not used by the Tauri command wrappers or `main()`

2. **Verify `run_command`**
    - Should be ~40 lines:
      ```rust
      #[tauri::command]
      async fn run_command(input: String, state: State<'_, AppState>) -> Result<CommandResponse, String> {
          let normalized = normalize_command_input(&input);
          let parsed = parse_command_input(&normalized);
          if !parsed.valid && !has_unknown_command(&parsed) {
              return Err(parsed.diagnostics.first().map(|d| d.message.clone()).unwrap_or_else(|| "invalid command".to_string()));
          }
          if let Some(response) = router::dispatch_desktop_command(&normalized, &parsed.normalized_tokens, state.clone()).await? {
              push_history(&state, &normalized, &response).await;
              return Ok(response);
          }
          let mut service = state.command_service.lock().await;
          Ok(service.execute_line(&normalized).await)
      }
      ```

3. **Verify `suggest_command_input`**
   - Should be ~10 lines delegating to `services::suggestions`

4. **Run `cargo check`**
   - No warnings about unused code
   - No compilation errors

5. **Run `make build`**
   - Desktop app builds successfully

---

## Cross-Cutting Concerns

### Tests

Many tests in `main.rs` test the functions being moved. When moving a function, move its tests too.

- `normalize_location_seed` tests → `utils.rs` tests or `services/vault_sync.rs` tests
- `extract_runebound_toml` tests → `services/vault_sync.rs` tests
- `dispatch` tests → `main.rs` tests (stay if they test Tauri wiring)

### `SoftDelete` and `EntityType`

These types might be used in `main.rs` by `suggest_command_input` or other functions. If they are moved to `commands/entity_commands.rs` or `services/`, make sure all references are updated.

### `EntitySuggestion` and `CommandSuggestion`

These are used by the autocomplete engine. If the autocomplete engine moves to `services/suggestions.rs`, these structs should move with it or be imported from it.

### `VaultReferenceEntry`, `ActiveReferenceQuery`, `PromptReferenceContext`

These are used by the vault reference system. Move them to `services/vault_sync.rs` or `utils/`.

### `SuggestionHelperText`

This is used by the autocomplete engine and rendered in the frontend. Keep it in `services/suggestions.rs` or `utils/`.

---

## Implementation Order for an Agent

An agent should implement **one phase at a time**, in order, and verify at each step:

1. Read `main.rs` and identify the functions/structs in the current phase
2. Create the target file (e.g., `services/entity_persistence.rs`)
3. Move the functions/structs, preserving logic exactly (no behavior changes)
4. Update `utils.rs` wrappers to point to the new location
5. Update `main.rs` to delete the old code and fix imports
6. Run `cargo check`
7. Run `cargo build` or `make build`
8. Verify the app still works (create npc, save, load, delete, autocomplete, etc.)
9. Move to the next phase

**Do not attempt multiple phases in one commit.** Each phase is large enough to be its own PR.

---

## Files That Will Be Created

| File | Phase | Purpose |
|---|---|---|
| `services/entity_persistence.rs` | 3 | `save_npc_draft`, `save_location_draft`, `save_faction_draft` |
| `services/vault_sync.rs` | 4 | `sync_database_from_vault`, `scan_*_row_from_markdown`, vault helpers |
| `services/suggestions.rs` | 5 | `suggest_command_input`, `build_command_suggestions`, autocomplete engine |
| `services/mod.rs` | 3-5 | Updated to declare new modules |

## Files That Will Be Heavily Modified

| File | Current Lines | Target Lines | Notes |
|---|---|---|---|
| `main.rs` | 139 | <500 | All business logic moved out; now pure Tauri wiring |
| `utils.rs` | 1,134 | <300 | Bridge types removed, only helpers remain |
| `services/ai_generation.rs` | 806 | ~1,000 | Add `reroll_*_field` methods |

## Files That Stay the Same

| File | Notes |
|---|---|
| `router.rs` | Already clean (51 lines) |
| `commands/*.rs` | Already clean, only import changes |
| `repositories/mod.rs` | Already clean, no changes needed |
| `app_state.rs` | Already clean, no changes needed |
| `runebound-models/` | Already clean, no changes needed |
| `core/` | Already clean, no changes needed |

---

*Last updated: 2026-06-14*  
*When a phase is complete, update this document with the new line counts and mark the phase as done.*
