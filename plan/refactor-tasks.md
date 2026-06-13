# Output Refactor Implementation Checklist

This checklist turns `plan/refactor.md` into concrete implementation work.

## Phase 0 - Baseline and Safety Net

- [ ] Create a short architecture decision record in `plan/refactor.md` section links for:
  - canonical output schema
  - command link strategy
  - parser authority boundaries
- [ ] Add baseline fixtures for current key command outputs:
  - `status`
  - `config test`
  - `config doctor`
  - setup/onboarding help text
- [ ] Add a temporary compatibility rule: no command should remove `output` text while migration is in progress.

## Phase 1 - Core Output Schema (Rust)

### 1.1 Add new output types

- [ ] Create a new module: `core/src/output.rs`.
- [ ] Define serializable types:
  - `OutputDoc`
  - `OutputBlock`
  - `InlineNode`
  - `StatusTone`
  - `SpinnerState`
- [ ] Re-export module in `core/src/lib.rs`.

### 1.2 Extend command response contract

- [ ] Add `output_doc: Option<OutputDoc>` to `CommandResponse` in `core/src/command.rs`.
- [ ] Keep existing fields (`output`, `error`, `segments`) untouched for compatibility.
- [ ] Ensure successful and error responses can set `output_doc`.

### 1.3 Builder utilities

- [ ] Add helper constructors in `core/src/output.rs` (or `core/src/output_builders.rs`):
  - `doc()`
  - `heading(level, text)`
  - `paragraph_text(text)`
  - `paragraph_with_inlines(inlines)`
  - `list(items)`
  - `code_block(language, text)`
  - `status(tone, text)`
  - `command_ref(label, command)`
- [ ] Add tests for JSON serialization of these nodes.

## Phase 2 - Command Output Migration (Rust)

### 2.1 High-impact commands first

- [ ] Update `execute_status` to produce structured `OutputDoc` while preserving plain `output` string.
- [ ] Update `config test` and `config doctor` formatting output with semantic blocks.
- [ ] Update first-time setup-required message to include explicit command refs (not plain suggestion text only).

### 2.2 Error path migration

- [ ] Ensure all `bail!`/error flows map to `status(error)` or equivalent block when response is built.
- [ ] Keep human-readable plain text mirrored in `error`/`output` during migration.

### 2.3 Contract tests

- [ ] Add tests validating `output_doc` shape for key commands.
- [ ] Add tests that command refs contain runnable command strings.

## Phase 3 - Tauri Bridge and TypeScript Contract

- [ ] Update Tauri command response typing at boundary (`desktop/src-tauri/src/main.rs` already serializes `CommandResponse`; verify compile + serde).
- [ ] Add TS types for `OutputDoc` and related node enums in `desktop/src/command/parser-client.ts` (or new `desktop/src/output/types.ts`).
- [ ] Update `CommandResponse` type in `desktop/src/App.tsx` to include `output_doc?: OutputDoc | null`.

## Phase 4 - Frontend Renderer Extraction

### 4.1 Extract renderer modules

- [ ] Create `desktop/src/output/types.ts` for typed output models.
- [ ] Create `desktop/src/output/renderer.tsx` for pure rendering from `OutputDoc`.
- [ ] Create `desktop/src/output/theme.ts` for semantic class mapping:
  - error (red)
  - info/helper (blue)
  - spinner (purple glyph + white text)
  - command ref (interactive accent)

### 4.2 Integrate renderer

- [ ] In `App.tsx`, render `output_doc` when present.
- [ ] Keep legacy line-based rendering as fallback while migration is incomplete.
- [ ] Add a clear adapter boundary: `responseToRenderableModel(response)`.

## Phase 5 - Remove Heuristic Command Guessing

- [ ] Stop using regex-based command extraction for migrated outputs:
  - deprecate `findClickableCommandInLine` path for `output_doc` entries
- [ ] Use only explicit `CommandRef` inline nodes for click-to-run when `output_doc` exists.
- [ ] Retain heuristic parser only for legacy fallback path during migration.

## Phase 6 - Input Normalization and Parsing Authority

### 6.1 Input normalization

- [ ] Add a shared normalization function in Rust (preferred) for user command lines:
  - unwrap `` `command` `` when entire input is wrapped
  - ignore surrounding whitespace
  - preserve quoted args content
- [ ] Call normalization before parse and execute.

### 6.2 Remove duplicated frontend validation

- [ ] Reduce/remove `isValidCommandLike` and related logic in `App.tsx` once backend-normalized parse info is sufficient.
- [ ] Ensure clickable behavior uses parser/manfiest truth, not local guessing.

## Phase 7 - Onboarding Flow Hardening

- [ ] Move onboarding command handling out of `App.tsx` into `desktop/src/onboarding/flow.ts`.
- [ ] Emit onboarding output using structured blocks where possible.
- [ ] Ensure all actionable onboarding instructions are explicit command refs.
- [ ] Normalize message severity usage:
  - validation failures -> error
  - tips and guidance -> info

## Phase 8 - Testing and Regression Coverage

### 8.1 Rust tests

- [ ] Add unit tests for output builders and node serialization.
- [ ] Add parse normalization tests for:
  - `` `help` ``
  - `` `config show` ``
  - malformed wrapping cases

### 8.2 Frontend tests

- [ ] Add renderer tests for:
  - headings
  - paragraphs
  - lists
  - command refs
  - spinner styles
  - error/info styles
- [ ] Add interaction tests ensuring `CommandRef` click executes expected command string.

### 8.3 Integration checks

- [ ] Verify command execution still works from keyboard input and click-to-run.
- [ ] Verify startup status check renders correctly with and without setup.
- [ ] Verify history commands and clear behavior remain unchanged.

## Phase 9 - Cleanup

- [ ] Remove obsolete legacy helpers when `output_doc` adoption is complete:
  - `segmentsToText`
  - heading line heuristic functions
  - heuristic clickable command matcher
- [ ] Remove unused `segments` field once no longer needed across UI and backend.
- [ ] Update docs in:
  - `plan/cli.md`
  - `plan/tech.md`
  - `plan/refactor.md`

## Execution Order (Recommended)

- [ ] Step 1: Phase 1 (schema + builders)
- [ ] Step 2: Phase 3 (TS contract plumbing)
- [ ] Step 3: Phase 4 (renderer extraction + fallback)
- [ ] Step 4: Phase 2 (migrate key command outputs to structured docs)
- [ ] Step 5: Phase 5 and 6 (kill heuristics + normalize input)
- [ ] Step 6: Phase 7 (onboarding extraction)
- [ ] Step 7: Phase 8 (tests) and Phase 9 (cleanup)

## Done Criteria Checklist

- [ ] Structured output drives rendering for all primary commands.
- [ ] No critical UX depends on regex parsing of plain output text.
- [ ] Markdown-wrapped commands like `` `help` `` execute correctly.
- [ ] Command links are explicit and reliably runnable.
- [ ] Error/info/spinner styling is semantic and consistent.
- [ ] Regression tests protect output and command interaction contracts.
