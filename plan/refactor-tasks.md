# Refactor Task Breakdown

This is the execution plan for `plan/refactor.md`, organized into agent-friendly tickets with dependencies and concrete deliverables.

## Milestone 1: Command Metadata Foundation

### T1.1 - Add command manifest model in core

- **Goal**: Create a single source of truth for command structure.
- **Scope**:
  - Add `core/src/command_manifest.rs`.
  - Define types for roots, subcommands, aliases, flags/options, value kinds, help metadata, examples.
  - Export `command_manifest()` from `core`.
- **Deliverables**:
  - New manifest types and builder function.
  - `core/src/lib.rs` exports manifest module.
- **Depends on**: none.
- **Validation**:
  - Unit test verifies manifest includes current commands (`status`, `config`, `exit`, MVP `npc` placeholders if applicable).

### T1.2 - Add manifest drift guard tests

- **Goal**: Prevent mismatch between clap commands and manifest.
- **Scope**:
  - Add tests comparing clap-defined command tree and manifest-defined command tree.
  - Fail fast on missing commands/subcommands/aliases.
- **Deliverables**:
  - `core` tests for manifest parity.
- **Depends on**: T1.1.
- **Validation**:
  - Test fails if a clap command is added without manifest update.

## Milestone 2: Shared Parse Contract

### T2.1 - Add parser contract types (AST-like)

- **Goal**: Standardize parse result used by execution + UI.
- **Scope**:
  - Add types such as `ParsedCommand`, `ParseDiagnostic`, `CursorContext`, `CompletionContext`.
  - Include normalized canonical command + alias resolution details.
- **Deliverables**:
  - New parse model module in `core` (e.g. `core/src/command_parse.rs`).
- **Depends on**: T1.1.
- **Validation**:
  - Unit tests for empty input, quoted input, escaped tokens, unknown subcommand, alias mapping.

### T2.2 - Implement parser entrypoint and normalization

- **Goal**: Provide a reusable parser for UI and backend pre-exec logic.
- **Scope**:
  - Implement `parse_command_input(input, manifest)`.
  - Reuse backend-compatible tokenization (`shell_words` behavior).
  - Include canonical target mapping (`history clear` -> `clear --history`, etc. if chosen canonical).
- **Deliverables**:
  - Parser function + tests.
- **Depends on**: T2.1.
- **Validation**:
  - Golden tests for tokenization and diagnostics.

## Milestone 3: Tauri Bridge APIs

### T3.1 - Add `get_command_manifest` Tauri command

- **Goal**: Make frontend consume backend command metadata.
- **Scope**:
  - In `desktop/src-tauri/src/main.rs`, add command returning serializable manifest.
  - Add serde derives to manifest types as needed.
- **Deliverables**:
  - New Tauri command and invoke handler registration.
- **Depends on**: T1.1.
- **Validation**:
  - Bridge test or manual invoke sanity check returns full manifest JSON.

### T3.2 - Add `parse_command_input` Tauri command

- **Goal**: Expose shared parse context to frontend.
- **Scope**:
  - Add tauri command wrapping core parser.
  - Return parse diagnostics and completion context.
- **Deliverables**:
  - New Tauri parser endpoint.
- **Depends on**: T2.2.
- **Validation**:
  - Known inputs return expected parsed structure.

## Milestone 4: Frontend Autocomplete Migration

### T4.1 - Create parser/manifest client module

- **Goal**: Isolate backend API calls from UI component code.
- **Scope**:
  - Add `desktop/src/command/parser-client.ts` with typed wrappers:
    - `loadManifest()`
    - `parseInput()`
- **Deliverables**:
  - Typed API client layer.
- **Depends on**: T3.1, T3.2.
- **Validation**:
  - Unit test with mocked `invoke`.

### T4.2 - Implement manifest-driven autocomplete engine

- **Goal**: Remove hardcoded command list logic.
- **Scope**:
  - Add `desktop/src/command/autocomplete.ts`.
  - Build suggestion generation from manifest + parse cursor context.
  - Add prefix indexing for efficient lookup.
- **Deliverables**:
  - New autocomplete module and tests.
- **Depends on**: T4.1.
- **Validation**:
  - New command in manifest appears in suggestions without editing autocomplete rules.

### T4.3 - Replace App.tsx hardcoded arrays and rule functions

- **Goal**: Complete migration away from command hardcoding.
- **Scope**:
  - Remove constants and logic tied to fixed lists/regex command assumptions.
  - Wire suggestion UI to new autocomplete engine.
- **Deliverables**:
  - Slimmed `App.tsx` command suggestion path.
- **Depends on**: T4.2.
- **Validation**:
  - Existing keyboard UX unchanged.

## Milestone 5: Structured Output Rendering

### T5.1 - Extend `CommandResponse` with structured output segments

- **Goal**: Make output semantics explicit and stable.
- **Scope**:
  - Add segment model to `CommandResponse` (while keeping `output` string fallback).
  - Segment kinds: text, command_ref, status/error.
- **Deliverables**:
  - Updated response type + serialization support.
- **Depends on**: none (can start earlier), but easiest after T3.
- **Validation**:
  - Backward compatibility maintained for plain `output` consumers.

### T5.2 - Emit structured segments from command handlers

- **Goal**: Move command-link semantics to backend.
- **Scope**:
  - Update help/status/history-producing handlers to emit command refs where relevant.
- **Deliverables**:
  - Segment-aware command output builders.
- **Depends on**: T5.1.
- **Validation**:
  - Help output has clickable command refs without regex guessing.

### T5.3 - Add dedicated output renderer module in frontend

- **Goal**: Stop relying on regex-first clickable detection.
- **Scope**:
  - Add `desktop/src/render/output.tsx`.
  - Render structured segments first.
  - Keep old regex as temporary fallback only.
- **Deliverables**:
  - New renderer path + tests.
- **Depends on**: T5.2.
- **Validation**:
  - Multiple clickable commands in one line are supported via segments.

## Milestone 6: App Decomposition + Runtime Efficiency

### T6.1 - Extract history and executor modules

- **Goal**: Reduce App monolith complexity.
- **Scope**:
  - Add `desktop/src/command/history.ts` and `desktop/src/command/executor.ts`.
  - Move history expansion, navigation, storage, and invocation orchestration out of `App.tsx`.
- **Deliverables**:
  - Smaller App with clear boundaries.
- **Depends on**: T4.3.
- **Validation**:
  - History behavior parity tests pass.

### T6.2 - Introduce reusable backend runtime resources

- **Goal**: Avoid repeated expensive setup in desktop mode.
- **Scope**:
  - Add shared app context in Tauri state for reusable resources where safe:
    - HTTP client
    - DB pool/lazy db handle
- **Deliverables**:
  - Refactored resource lifecycle management.
- **Depends on**: none strict, but easiest after parse/manifest endpoints stabilize.
- **Validation**:
  - No behavior regressions; reduced repeated initialization overhead.

## Milestone 7: Cleanup and Legacy Removal

### T7.1 - Remove legacy command-regex authority paths

- **Goal**: Ensure only one command truth remains.
- **Scope**:
  - Delete regex-driven command validity/autocomplete authority logic from `App.tsx`.
  - Keep minimal fallback only for non-structured legacy lines if still needed.
- **Deliverables**:
  - Legacy logic removed.
- **Depends on**: T4.3, T5.3.
- **Validation**:
  - No command list duplication remains in frontend.

### T7.2 - Final regression and docs sync

- **Goal**: Lock in new architecture and guardrails.
- **Scope**:
  - Update docs in `plan/` as needed.
  - Ensure tests cover command addition workflow.
  - Run full build/test checks.
- **Deliverables**:
  - Updated docs and stable CI baseline.
- **Depends on**: all prior tasks.
- **Validation**:
  - `make build` passes.
  - New command dry-run shows no frontend code edits required for autocomplete/clickability.

## Suggested Agent Parallelization

- Parallel track A (core): T1.1 -> T1.2 -> T2.1 -> T2.2 -> T5.1 -> T5.2
- Parallel track B (tauri bridge): T3.1 + T3.2 (after corresponding core pieces)
- Parallel track C (frontend): T4.1 -> T4.2 -> T4.3 -> T5.3 -> T6.1
- Parallel track D (performance/cleanup): T6.2 -> T7.1 -> T7.2

## Quick Start Order (if doing one ticket at a time)

1. T1.1
2. T1.2
3. T2.1
4. T2.2
5. T3.1
6. T3.2
7. T4.1
8. T4.2
9. T4.3
10. T5.1
11. T5.2
12. T5.3
13. T6.1
14. T6.2
15. T7.1
16. T7.2
