# Phase 3 - Modular Desktop Execution Pipeline

## Current State (After Initial Work)

### Completed
- Created `services/ai_generation.rs` with `AiGenerationService` (AI generation extracted)
- Created `repositories/mod.rs` with `VaultRepository` trait and `ProdVaultRepository`
- Created `utils.rs` with shared types (NpcSeed, SaveNpcDraftInput, RerollNpcFieldInput, etc.)
- Created `commands/` directory with modular handlers (npc_commands, location_commands, faction_commands, entity_commands, system_commands, create_commands) - these exist but are not yet integrated
- Project compiles successfully (1 error fixed: NpcSeed needed Serialize)

### What's Broken
The command modules in `commands/` are disconnected stubs:
1. They import from `crate::utils` but types don't match what commands expect
2. Example mismatches: `EnsureLocationInput` missing `name` field, `LocationRerollContext` missing fields like `kind_custom`, `exports`, `current_tension`
3. Functions like `reroll_npc_field`, `save_npc_draft_impl`, `resolve_entity` return errors (stub implementations)
4. `mod commands` was removed from main.rs because it wouldn't compile

### Root Cause
The command modules were created by copying code from router.rs/main.rs and using `super::super::main` imports, which doesn't work in Rust binary crates. When we switched to `crate::utils`, the types weren't properly aligned with what the command handlers actually need.

## Plan to Continue

### Phase 3.1: Fix Utils Types (Critical Path)
The utils.rs types are the foundation for all command modules. They must match what handlers expect.

1. **Fix EnsureLocationInput and EnsureLocationResult**
   - Add `name: String` field to `EnsureLocationInput`
   - Ensure `ensure_location_exists()` function has correct signature

2. **Fix LocationRerollContext and RerollLocationFieldResult**
   - Add missing fields: `kind_custom`, `exports`, `current_tension`
   - Add `list_value` field to `RerollLocationFieldResult`

3. **Fix FactionRerollContext and RerollFactionFieldResult**
   - Add missing fields: `kind_custom`, `leadership`, `headquarters`, `sphere_of_influence`, `resources_assets`, `allies`, `rivals_enemies`, `current_tension`, `goals_short_term`, `goals_long_term`, `symbol_description`
   - Add `list_value` field to `RerollFactionFieldResult`

4. **Fix Save*DraftInput and Save*DraftResult structs**
   - Ensure SaveLocationDraftResult has `slug` field
   - Ensure SaveFactionDraftInput has `slug` and `vault_path` fields
   - Ensure SaveFactionDraftResult has `slug` field

5. **Fix SoftDeleteEntityInput and results**
   - Add `target: String` field to `SoftDeleteEntityInput`
   - Update SoftDeleteEntityResult with proper fields: `entity_type`, `name`, `slug`, `trash_vault_path`, `id`
   - Update UndoSoftDeleteResult with proper fields: `entity_type`, `name`, `id`

### Phase 3.2: Implement Stub Functions in Utils
Once types are correct, implement the stub functions:

1. **reroll_npc_field** - Wrap the actual reroll logic from main.rs
2. **reroll_location_field** - Similar wrapping
3. **reroll_faction_field** - Similar wrapping
4. **save_npc_draft_impl** - Delegate to main.rs implementation
5. **save_location_draft_impl** - Delegate to main.rs implementation
6. **save_faction_draft_impl** - Delegate to main.rs implementation
7. **ensure_location_exists** - Delegate to main.rs implementation
8. **resolve_entity** - Delegate to main.rs implementation
9. **soft_delete_entity** - Delegate to main.rs implementation
10. **undo_last_soft_delete** - Delegate to main.rs implementation

### Phase 3.3: Re-integrate Command Modules
1. Add `mod commands;` back to main.rs
2. Fix remaining import issues in command modules
3. Ensure all handler functions have correct signatures matching utils types
4. Add missing helper functions to command modules (like `canonical_faction_reroll_field` which is defined locally in faction_commands.rs but also exists in utils)

### Phase 3.4: Connect Router to Command Registry
1. Modify router.rs to use `commands::desktop_handler_registry()` instead of its own inline handlers
2. Keep backward compatibility by ensuring router.rs handlers delegate to command modules
3. Eventually remove duplicate handler code from router.rs

### Phase 3.5: Add Repository Implementations
1. Fix SqlitePool import issue - either use `dnd_core::db::Database` type or add sqlx to Cargo.toml
2. Uncomment database repository traits and implementations
3. Create mock repository implementations for testing

### Phase 3.6: Testing Infrastructure
1. Create `tests/` directory with mock repository implementations
2. Add unit tests for command handlers using mocked repositories
3. Verify AI generation service can be tested independently

## Implementation Order

```
Phase 3.1 (Critical)          Phase 3.2          Phase 3.3
┌─────────────────────┐      ┌────────────┐     ┌──────────────┐
│ Fix utils.rs types  │ ───► │ Implement  │ ──► │ Re-integrate │
│ to match handlers   │      │ stubs      │     │ commands mod │
└─────────────────────┘      └────────────┘     └──────────────┘
                                                            │
                                                            ▼
Phase 3.4                   Phase 3.5                  Phase 3.6
┌──────────────────┐        ┌──────────────┐        ┌──────────────┐
│ Connect router   │ ─────► │ Add repo     │ ─────► │ Add tests    │
│ to command reg   │        │ impls        │        │              │
└──────────────────┘        └──────────────┘        └──────────────┘
```

## Key Files to Modify

| File | Purpose |
|------|---------|
| `src/utils.rs` | Foundation types and stub functions |
| `src/commands/npc_commands.rs` | NPC handler operations |
| `src/commands/location_commands.rs` | Location handler operations |
| `src/commands/faction_commands.rs` | Faction handler operations |
| `src/commands/entity_commands.rs` | Load/show/preview/delete/undo |
| `src/commands/system_commands.rs` | Save/reroll/cancel |
| `src/commands/create_commands.rs` | Create npc/location/faction |
| `src/commands/mod.rs` | Handler registry (already exists) |
| `src/repositories/mod.rs` | Repository traits (already exists, needs SqlitePool fix) |
| `src/router.rs` | Should delegate to commands module |

## Constraints
- Rust 2024 edition with native async traits
- Inject repositories via AppState using `Arc<dyn Trait>` pattern
- Build on Phase 1 registry (`command_handler`, `command_specs` crates)
- Keep original router.rs handlers working until new modules are verified

## Notes
- Original router.rs handlers still work - they weren't removed, just the new command modules weren't connected
- The `commands::desktop_handler_registry()` already exists and has all 15 handlers registered
- When connected, the new registry will replace router.rs's inline handlers