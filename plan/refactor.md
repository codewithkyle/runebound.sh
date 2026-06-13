# Output Rendering and Command UX Refactor Plan

## Context

The desktop app currently renders terminal-like output (text, headings, spinners, and command hints) by mixing string parsing, UI logic, and command execution logic in one large component.

This document captures:

- The major problems in the current implementation.
- Why these problems keep causing regressions.
- A recommended architecture that is idiomatic, extendable, and testable.
- A phased migration strategy to get there safely.

## Current State (High Level)

### Backend output shape

- Commands return `CommandResponse { ok, output, error, exit_code, segments }`.
- `segments` exist but are minimally typed (`text` or `error`) and not used for rich rendering.
- Most output is still generated as plain newline-delimited strings.

### Frontend rendering approach

- `App.tsx` contains command execution, onboarding flow, output rendering, command detection, and interactivity.
- Output rendering is largely line-based and heuristic:
  - heading detection from string patterns
  - clickable command extraction from regex patterns
  - spinner styling from first character matching a spinner frame set

### Command parsing approach

- Parsing authority is split:
  - Rust Clap parser in `core`
  - Rust manifest parser helper
  - Frontend regex checks and command-like validation
- Some command-like strings in output are parsed as clickable commands only by heuristics.

## Problems

### 1) Output contract is too weak

- The backend mostly emits strings and only coarse segments.
- The UI must infer semantics from text instead of receiving explicit semantics.
- This makes formatting changes dangerous because behavior depends on exact wording.

### 2) Renderer behavior depends on fragile heuristics

- Heading detection is pattern-based, not structural.
- Clickable command extraction tries multiple regex paths and can miss valid commands.
- Backtick command formatting (for example `` `help` ``) may display but not execute as expected.
- Only first backtick command occurrence in a line is considered in current matching flow.

### 3) Parsing logic is duplicated

- Command validity and command-like detection exist in frontend helper logic.
- Backend parser and frontend parser assumptions can drift over time.
- Drift creates bugs where autocomplete, click-to-run, and execution disagree.

### 4) Command definitions are not first-class for output linking

- Commands are known by manifest, but output command linking still relies on inferred text.
- There is no explicit command reference node in output payloads.

### 5) Monolithic component causes broad blast radius

- `App.tsx` is large and does too many jobs.
- Changes in onboarding, render rules, or parsing can break unrelated concerns.
- Unit testing is difficult because responsibilities are not isolated.

### 6) Styling semantics are inconsistent

- There are color tokens and utility classes, but message semantics are not centralized.
- Error/info/success/spinner rendering is partly convention and partly ad hoc.
- Spinner visual requirement (purple glyph + white text) is not encoded as a strict semantic component contract.

### 7) Insufficient automated coverage for output behavior

- There are parser/manfiest tests in Rust, but little or no frontend contract testing for render and click behavior.
- Regressions happen where output text changes without tests failing.

## Refactor Goals

1. Make output rendering deterministic and schema-driven.
2. Make command invocation links explicit and reliable.
3. Keep parser authority in one place.
4. Preserve terminal-inspired UX while improving maintainability.
5. Reduce regressions with focused tests around render contracts.

## Recommended Architecture

### A) Introduce a structured output document model

Add a new output payload in `core` that represents semantic UI blocks.

Suggested shape (illustrative):

```rust
pub struct OutputDoc {
    pub blocks: Vec<OutputBlock>,
}

pub enum OutputBlock {
    Heading { level: u8, text: String },
    Paragraph { inlines: Vec<InlineNode> },
    List { items: Vec<Vec<InlineNode>> },
    Code { language: Option<String>, text: String },
    Status { tone: StatusTone, text: String },
    Spinner { state: SpinnerState, text: String },
}

pub enum InlineNode {
    Text(String),
    CommandRef { label: String, command: String },
    Emphasis(String),
    Strong(String),
    Code(String),
}
```

Notes:

- Keep `output` string temporarily for compatibility and CLI-like fallback.
- Use `OutputDoc` as the canonical source for desktop rendering.

### B) Add output builder utilities in Rust

Create helper APIs so command authors do not manually craft brittle strings.

Examples:

- `out::heading(2, "System Status")`
- `out::error("vault.path is not configured")`
- `out::info("Type setup help to continue")`
- `out::command("status")`
- `out::spinner_running("Checking Ollama")`

Benefits:

- Consistent semantics and style mapping.
- Faster, safer command implementation.
- Lower chance of regressions from string edits.

### C) Make command links explicit (no regex guessing)

Use inline `CommandRef` nodes instead of searching rendered text for possible commands.

If markdown-compatible links are preferred, reserve a URI scheme:

- `[config show](cmd:config%20show)`

and parse that into `CommandRef` nodes before render.

### D) Define a lightweight markdown-inspired grammar

Support a strict subset only (enough for terminal-like UX):

- headings (`#`, `##`, `###`)
- paragraphs
- bullet lists
- inline code
- command links (`cmd:`)
- fenced code blocks

Do not support full markdown initially. Keep the grammar intentionally constrained for predictable rendering.

### E) Centralize command normalization and parsing

Introduce a shared normalization step before parse/execute:

- Trim wrapped single-token markdown code formatting:
  - `` `help` `` -> `help`
- Preserve quoted args and shell-like splitting behavior.

Then rely on backend parse result as source of truth for command validity and completion context.

### F) Split frontend into focused modules

Refactor `App.tsx` into composable units:

- `output/model.ts` - output doc TS types and adapters
- `output/renderer.tsx` - pure render logic from typed blocks
- `output/theme.ts` - semantic class mapping (`error`, `info`, `spinner`, `command`)
- `command/execute.ts` - invocation + response handling
- `command/input-normalize.ts` - pre-parse normalization
- `onboarding/flow.ts` - onboarding command state machine

### G) Establish semantic style contracts

Map output semantics to stable style tokens/classes:

- `error` -> red
- `info` -> blue
- `spinner` -> purple glyph + white text
- `command-ref` -> interactive command styling (underline + accent)

The renderer should use semantic classes only, not ad hoc inline color decisions.

## Test Strategy

### Backend

- Unit tests for output builders and serialization.
- Parser normalization tests for markdown-wrapped command inputs.
- Snapshot-like tests for command responses converted to `OutputDoc`.

### Frontend

- Renderer tests from `OutputDoc` fixtures:
  - headings
  - lists
  - command refs
  - spinner states
  - error/info styles
- Interaction tests:
  - clicking a `CommandRef` executes expected command
  - markdown code-wrapped input normalization behavior

### Integration

- Tauri contract tests for payload compatibility.
- Golden fixtures for a few key commands (`status`, `config test`, onboarding prompts).

## Migration Plan

### Phase 1: Add new output schema (non-breaking)

- Add `output_doc` field to `CommandResponse`.
- Keep existing `output` and `segments` for compatibility.

### Phase 2: Backend starts emitting structured docs

- Update key commands (`status`, `config test`, `config doctor`) to produce `OutputDoc`.
- Keep plain text output synchronized during transition.

### Phase 3: Frontend renderer switch

- Implement typed renderer for `OutputDoc`.
- Use old line parser only as fallback when `output_doc` is absent.

### Phase 4: Remove heuristic command detection

- Stop regex-based command extraction from lines.
- Drive clickability purely from explicit `CommandRef` nodes.

### Phase 5: Unify parsing and normalization

- Add one normalization pipeline before parse and execute.
- Remove frontend-only command validity heuristics.

### Phase 6: Cleanup and hardening

- Delete deprecated render helpers and unused segment logic.
- Expand regression tests around output and command interactions.

## Risks and Mitigations

### Risk: temporary dual-format complexity

Mitigation:

- Keep transition short and scoped.
- Add feature flags or adapter boundaries.

### Risk: behavior mismatch during migration

Mitigation:

- Add command fixture snapshots for old vs new outputs.
- Roll out command-by-command, starting with status/help flows.

### Risk: over-designing markdown support

Mitigation:

- Keep grammar minimal and purpose-built.
- Expand only for concrete product needs.

## Definition of Done

- Output rendering is based on explicit typed nodes, not text heuristics.
- Command links are explicit and always executable when shown as actionable.
- Markdown-wrapped command inputs (for example `` `help` ``) normalize correctly.
- Frontend no longer duplicates command validity logic.
- Styling of error/info/spinner is consistent and semantically mapped.
- Contract tests exist for output rendering and command click behavior.
