# Refactor Plan for Command Extensibility

This checklist defines what MUST be implemented so command growth does not require fragile, duplicated edits across Rust and Solid UI.

## Non-Negotiable Outcomes

- [ ] Command definitions have a single source of truth.
- [ ] Desktop autocomplete/typeahead is generated from command metadata, not hardcoded arrays.
- [ ] Command parsing behavior is consistent between TUI and desktop (including quoted args).
- [ ] Clickable output uses structured metadata, not regex-only heuristics.
- [ ] `desktop/src/App.tsx` is decomposed into maintainable modules.
- [ ] Adding a new command does not require editing multiple unrelated parser/suggestion/render helpers.

## MUST-Do Checklist (Implementation + Acceptance)

### 1) Create a Command Manifest in Core

- [ ] Introduce a command metadata model in `core` (new module like `core/src/command_manifest.rs`).
- [ ] Manifest MUST represent: root command, subcommands, aliases, flags/options, value expectations, help text, examples.
- [ ] Manifest MUST include completion hints:
  - static choices
  - dynamic provider key (for future runtime suggestions, e.g. NPC ids)
- [ ] Add core API to return manifest data (`pub fn command_manifest() -> CommandManifest`).

Acceptance:

- [ ] Every command present in `clap` definitions is present in manifest.
- [ ] A CI/test check fails if clap and manifest drift.

### 2) Centralize Parsing Contract (AST-like Parse Result)

- [ ] Add a parse result model in `core` (e.g. `ParsedCommand`), including:
  - root/subcommand
  - parsed options/args
  - cursor/token context for autocomplete
  - diagnostics/errors
- [ ] Add a parser entrypoint in `core` that can be used for:
  - execution parse
  - autocomplete parse
  - command validation for clickable output
- [ ] Ensure parser/tokenizer handles quoted strings and escapes consistently.

Acceptance:

- [ ] Desktop autocomplete and backend execution rely on the same parse contract.
- [ ] Cases like quoted values produce identical parse behavior in desktop and TUI.

### 3) Expose Manifest + Parse via Tauri Commands

- [ ] In `desktop/src-tauri/src/main.rs`, add commands:
  - `get_command_manifest`
  - `parse_command_input`
- [ ] Keep `run_command` for execution, but route pre-execution UI logic through these new commands.

Acceptance:

- [ ] Frontend has zero hardcoded authoritative command lists.
- [ ] Frontend can build suggestions from manifest and parse context only.

### 4) Replace Hardcoded Frontend Command Tables

- [ ] Remove command and flag constants in `desktop/src/App.tsx` as authority:
  - `TOP_LEVEL_COMMANDS`
  - `CONFIG_SUBCOMMANDS`
  - `NPC_SUBCOMMANDS`
  - `CONFIG_INIT_FLAGS`
  - `CLEAR_FLAGS`
  - `HISTORY_SUBCOMMANDS`
- [ ] Build suggestion engine from manifest + parse cursor context.
- [ ] Keep UI-specific aliases only if they are also declared in manifest.

Acceptance:

- [ ] Adding a new command in core metadata automatically appears in desktop suggestions.

### 5) Structured Output Rendering Contract

- [ ] Extend `CommandResponse` in `core/src/command.rs` to support structured segments, for example:
  - text segment
  - command segment (`command_ref`)
  - path/resource segment
  - status level (`info`, `error`, `success`)
- [ ] Preserve existing `output: String` during migration for compatibility.
- [ ] Update command handlers to emit semantic segments for help/history output.

Acceptance:

- [ ] Desktop clickability works from structured command segments first.
- [ ] Regex fallback is only legacy fallback, not the primary mechanism.

### 6) Refactor Frontend into Focused Modules

- [ ] Split `desktop/src/App.tsx` into modules (suggested):
  - `desktop/src/command/history.ts`
  - `desktop/src/command/autocomplete.ts`
  - `desktop/src/command/parser-client.ts`
  - `desktop/src/command/executor.ts`
  - `desktop/src/render/output.tsx`
- [ ] App component MUST remain orchestration-focused (state wiring + layout).
- [ ] Each module MUST have clear input/output contracts and no hidden cross-module assumptions.

Acceptance:

- [ ] Core command logic is testable without mounting the full App component.

### 7) Alias and Help Behavior Standardization

- [ ] Define aliases in one place (manifest/core), including canonical command mapping.
- [ ] Ensure `-h`, `--help`, and `<cmd> help` behavior is represented in manifest metadata.
- [ ] Ensure clickable root commands that require subcommands resolve to canonical help target (e.g. `config --help`).

Acceptance:

- [ ] Alias behavior in autocomplete, validation, and execution is consistent.

### 8) Performance and Resource Lifecycle

- [ ] In long-lived desktop runtime, avoid rebuilding expensive resources repeatedly:
  - shared/lazy `reqwest::Client`
  - shared/lazy DB pool or app context where safe
- [ ] Optimize suggestion lookup with indexed structures (prefix map/trie) built from manifest.
- [ ] Avoid repeated per-render line parsing where possible; pre-process output entries once on append.

Acceptance:

- [ ] Input latency remains stable as command count grows.

### 9) Testing Requirements (MUST add)

- [ ] Core tests:
  - manifest completeness and drift checks
  - parser/tokenization edge cases
  - alias normalization behavior
  - structured output segment generation
- [ ] Frontend tests:
  - autocomplete from manifest
  - keyboard behavior (`Enter`, `Tab`, arrows, `Ctrl+C`)
  - clickable output from structured segments
- [ ] Integration tests (Tauri bridge):
  - `get_command_manifest` schema
  - `parse_command_input` parity with execution outcomes

Acceptance:

- [ ] `make build` and tests pass with command-addition regression coverage.

## Implementation Notes by File Area

- `core/src/command.rs`
  - keep clap-based execution
  - add normalization hooks so aliases resolve before handler dispatch
  - evolve `CommandResponse` to include structured payload

- `core/src/lib.rs`
  - export new manifest/parser modules

- `desktop/src-tauri/src/main.rs`
  - add manifest/parse command handlers
  - keep execution handler lean and deterministic

- `desktop/src/App.tsx`
  - remove embedded parser/command truth
  - consume manifest and parse endpoints through a thin client wrapper

## Suggested Rollout Order

- [ ] Step 1: Land manifest model + bridge endpoint (no frontend behavior change yet)
- [ ] Step 2: Switch autocomplete to manifest-driven suggestions
- [ ] Step 3: Land parse endpoint and migrate frontend validation/click target resolution
- [ ] Step 4: Add structured output segments and migrate renderer
- [ ] Step 5: Remove legacy regex-first and hardcoded command fallback paths

## Definition of Done

- [ ] A new command can be added by editing core command definitions/metadata only.
- [ ] Desktop suggestion/click/help behaviors update without manual regex/list edits.
- [ ] Parser behavior is consistent across TUI and desktop for normal and quoted inputs.
- [ ] Output rendering remains stable when help text format evolves.
