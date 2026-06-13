# Phase 1 Plan

## Objective

Deliver a runnable Rust CLI (no Tauri yet) with first-time config setup, SQLite bootstrapping, and vault folder/file I/O foundations.

## In Scope

- Build and run the program as a CLI app.
- Use `clap` for command parsing.
- Use `sqlx` with SQLite and bootstrap initial database schema.
- Require and guide first-time setup before normal operations.
- Read/write files in the user-configured Obsidian vault.
- Ensure top-level vault folders exist for:
  - `npcs`
  - `locations`
  - `items`
  - `factions`

## Out of Scope

- NPC generation logic
- System prompt design
- LLM-powered create flows
- Tauri desktop shell

## Implementation Plan

1. Workspace and crate bootstrap
   - Initialize Rust workspace with `core` and `tui` crates.
   - Add baseline dependencies for `clap`, `sqlx`, and async runtime.
   - Add a minimal `main` entrypoint that starts CLI command handling.

2. Config system and first-time setup
   - Implement config loading from global/workspace TOML files.
   - Detect missing required settings on startup.
   - Launch interactive `config init` wizard when config is missing/invalid.
   - Collect and validate vault path + Ollama base URL + default model.
   - Save config and print setup summary.

3. Vault initialization and file I/O layer
   - Implement vault root resolver based on effective config.
   - Create missing top-level folders: `npcs`, `locations`, `items`, `factions`.
   - Implement safe read/write helpers scoped to vault root.
   - Block path traversal outside vault root.

4. SQLite bootstrap with `sqlx`
   - Create/open SQLite database in app data location.
   - Add migrations and apply on startup.
   - Create baseline tables for documents/index metadata needed in later phases.
   - Add a basic health check command to verify DB connectivity.

5. CLI command surface for phase validation
   - Implement minimal commands needed to validate setup:
     - `config init`, `config show`, `config test`, `config doctor`
   - Ensure startup requires valid config before non-config commands run.
   - Return concise, actionable error messages.

6. Verification and acceptance checks
   - Build succeeds locally.
   - CLI runs and enters setup flow on fresh machine state.
   - Config persists and reloads correctly on second launch.
   - Vault folders are created automatically.
   - SQLite file is created and migrations apply successfully.

## Phase 1 Checklist

- [x] Rust workspace created with `core` and `tui` crates
- [x] `clap` integrated for command parsing
- [x] `sqlx` + SQLite dependency integrated and build passes
- [x] SQLite database bootstrap implemented
- [x] Initial migrations added and auto-applied
- [x] Config loader for global/workspace TOML implemented
- [x] First-time setup wizard implemented and required
- [x] Setup prompts for vault path, Ollama URL, and model
- [x] Config validation implemented for required keys and path/url sanity
- [x] Config saved and reloaded correctly on restart
- [x] Vault read/write service implemented
- [x] Vault top-level folders auto-created (`npcs`, `locations`, `items`, `factions`)
- [x] Path traversal protections in vault access layer
- [x] `config init`, `config show`, `config test`, `config doctor` available
- [x] Program can be built and run as CLI without Tauri

## Exit Criteria

Phase 1 is complete when a new user can install and run the CLI, complete guided first-time setup, and end with a valid config, initialized SQLite database, and initialized Obsidian vault folder structure.
