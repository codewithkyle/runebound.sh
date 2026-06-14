# Phase 3 - Modular Desktop Execution Pipeline

## Current State (After Initial Work)

### Completed
- Created `services/ai_generation.rs` with `AiGenerationService` (AI generation extracted)
- Created `repositories/mod.rs` with `VaultRepository` trait and `ProdVaultRepository`
- Created `commands/` directory with modular handlers (npc_commands, location_commands, faction_commands, entity_commands, system_commands, create_commands)
- Project compiles successfully (1 error fixed: NpcSeed needed Serialize)
- **Phase 3.1** – All shared types in `desktop/src-tauri/src/utils.rs` now match the router/command expectations (EnsureLocation*, reroll contexts/results, save/soft-delete structs, shared `EntityDetails`).
- **Phase 3.2** – Every stub helper in `utils.rs` delegates to the production logic from `main.rs`, so command modules can call reroll/save/ensure/resolve/delete without panicking.

### What's Broken
The command modules in `commands/` are still disconnected:
1. `mod commands` is still commented out/absent from `main.rs`, so none of the handlers are reachable.
2. `router.rs` still hosts the legacy inline handlers instead of delegating to `commands::desktop_handler_registry()`, so we have duplicate logic and no single registry entry point.

### Root Cause
The command modules were created by copying code from router.rs/main.rs and using `super::super::main` imports, which doesn't work in Rust binary crates. When we switched to `crate::utils`, the types weren't properly aligned with what the command handlers actually need.

## Plan to Continue

### Phase 3.1: Fix Utils Types (Completed ✅)
All shared structs/enums in `desktop/src-tauri/src/utils.rs` now mirror the router/command expectations and include `EntityDetails` conversions.

### Phase 3.2: Implement Stub Functions in Utils (Completed ✅)
All helper functions in `utils.rs` delegate to the production logic extracted from `main.rs`, so command modules can safely use them.

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
