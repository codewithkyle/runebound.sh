# MVP Review: CLI, Typeahead, AST, Autocomplete, Output Rendering

## Scope Reviewed

- `core/src/command.rs`
- `tui/src/main.rs`
- `desktop/src/App.tsx`
- `desktop/src-tauri/src/main.rs`
- Supporting config/health/vault/db paths where they affect command execution and rendering behavior

## Executive Takeaways

- The POC works, but command semantics are duplicated in multiple places (Rust backend + Solid UI), which creates drift risk as MVP features expand.
- The desktop app currently relies on string/regex heuristics for both autocomplete and clickable output parsing; this is fragile and expensive to maintain.
- There is no shared command grammar/AST layer today. MVP should introduce one and use it as the source for parsing, validation, completion, and clickable output targets.
- `desktop/src/App.tsx` is a monolith (UI + parser + autocomplete + output interpretation + history). Splitting this now will lower regression risk.

## Detailed Findings

### 1) CLI and Command Path

#### What is good

- `clap` command definitions in `core/src/command.rs` are a strong base for idiomatic Rust CLI evolution.
- Tauri command bridge is simple (`run_command` invokes `dnd_core::command::execute_line`) and keeps the parser in Rust.

#### Efficiency concerns

- `execute_status`, health checks, and init flows repeatedly initialize DB and HTTP clients. This is acceptable for CLI process-per-run but suboptimal for long-lived desktop sessions.
- Setup/init logic exists in two places (`core` non-interactive init and `tui` setup wizard path), increasing duplicate validation and I/O behavior.

#### Extensibility concerns

- UI command behavior is partly client-local (`clear`, `history`, `exit`) while core handles other commands. Without a shared command manifest, help/typeahead/rendering can drift.
- Command aliases are implemented manually in UI (`history clear` vs `clear --history`) rather than defined in a central command schema.

### 2) Typeahead and Autocomplete Path

#### Current state

- Autocomplete in `desktop/src/App.tsx` is hardcoded via arrays and helper functions:
  - `TOP_LEVEL_COMMANDS`, `CONFIG_SUBCOMMANDS`, `NPC_SUBCOMMANDS`, flags arrays
  - `buildSuggestions`, `buildSubcommandSuggestions`, `buildFlagSuggestions`

#### Efficiency concerns

- Suggestions re-tokenize and re-filter on every keypress with repeated `split(/\s+/)` and case normalization.
- There is no indexed structure (trie/prefix map/command graph), so all suggestions are linear scans of arrays.

#### Extensibility concerns

- Hardcoded command lists must be manually synchronized with Rust clap definitions.
- Flag suggestion logic cannot scale to typed arg values, dynamic resource suggestions (e.g., NPC ids), or context-sensitive completions without expanding ad hoc branches.
- Tokenization differs from backend parser (`split(/\s+/)` in UI vs `shell_words::split` in Rust), so quoted arguments and edge cases will diverge.

### 3) AST / Parsing Path

#### Current state

- There is no explicit command AST in either frontend or backend shared layer.
- Frontend uses lightweight validators (`isValidCommandLike`) and regex matching for command-like text.

#### Risks

- Validation rules for clickability and suggestions are hand-coded separately from execution rules.
- Complex commands (quoted refs, future expression-like args, nested subcommands/options) will become difficult to support consistently.

#### MVP need

- Introduce a small command grammar AST and parser contract used for:
  - input tokenization
  - parse/validate
  - autocomplete context
  - clickable command target resolution

### 4) Output Rendering Path

#### Current state

- Output is returned as plain strings and interpreted in UI using regex heuristics (`findClickableCommandInLine`, usage/history/backtick matching).
- Rendering scans each output line and returns only one clickable match per line.

#### Efficiency concerns

- Regex heuristics run per rendered line and repeat work during reactive rerenders.
- Output is split by newline in the render path (`entry.text.split("\n")`), causing repeated allocations.

#### Extensibility concerns

- Clickability is pattern-based, not semantic. New help formats can silently break click targets.
- Only first matching token per line is clickable; cannot represent rich interactive output.
- Command token regex is hardcoded and quickly goes stale as command set grows.

## Architectural Recommendations for MVP

### A) Single Source of Truth for Commands (highest priority)

- Build a command manifest from Rust core definitions (or maintain a typed manifest in core that clap and UI both consume).
- Manifest should describe:
  - command roots, subcommands, aliases
  - flags/options (name, arity, value kind)
  - help summaries and examples
  - completion metadata hooks (static list vs dynamic provider)
- UI should request/consume manifest at startup instead of hardcoding arrays/regex token lists.

### B) Shared Parser + AST Contract

- Add parser outputs such as:
  - `ParsedCommand { root, subcommand, options, position_context, is_complete }`
  - parse diagnostics for friendly errors
- Ensure frontend tokenization mirrors backend behavior (quotes/escaping) by sharing parser logic from Rust or exposing parse endpoint through Tauri.
- Use AST context to drive autocomplete and click resolution, removing duplicated rule branches.

### C) Structured Output Model (replace regex-first rendering)

- Evolve command response from plain text to structured blocks/segments, e.g.:
  - `text`
  - `command_ref` (click runs this)
  - `path_ref`
  - `status/error`
- Keep plain-text fallback for terminal compatibility, but prefer structured payload in desktop.
- Move linkification responsibility to backend where command semantics are known.

### D) Decouple Frontend Modules

- Split `desktop/src/App.tsx` into focused modules:
  - `command-history`
  - `input-parser` (or bridge client)
  - `autocomplete-engine`
  - `output-renderer`
  - `command-executor`
- Keep App component mostly orchestration + layout.

### E) Performance Baseline Improvements

- Cache/memoize tokenization per input string.
- Precompute suggestion indices from manifest (prefix maps or trie) instead of filtering raw arrays each keypress.
- For desktop runtime, consider persistent shared resources in Tauri state:
  - reusable `reqwest::Client`
  - reusable DB pool or lazily initialized app context

## Concrete Hardcoding to Remove Early

- Command arrays and flag arrays in `desktop/src/App.tsx` (`TOP_LEVEL_COMMANDS`, `*_SUBCOMMANDS`, `*_FLAGS`).
- Inline clickable token regex in `findClickableCommandInLine`.
- Manual command validity rules in `isValidCommandLike`.
- Alias handling hardcoded only in UI (`history clear` and `clear --history`) without a shared alias definition.

## Suggested Implementation Phases

### Phase 1: Safety + Sync

- Introduce command manifest endpoint in core/Tauri.
- Replace hardcoded autocomplete lists with manifest-driven lists.
- Replace command validity checks with manifest/parser-backed checks.

### Phase 2: Parser/AST

- Add parse endpoint returning AST + cursor context + diagnostics.
- Use AST context for suggestions and click target validation.

### Phase 3: Output Contract

- Add structured output segments to `CommandResponse` while preserving `output` string fallback.
- Update desktop renderer to consume segments first, regex fallback second.

### Phase 4: Runtime Efficiency

- Centralize reusable services in app state (DB/client).
- Remove duplicated init/setup validation paths where possible.

## Success Criteria for the Refactor

- Adding a command in Rust updates desktop autocomplete/help/clickability without editing `App.tsx` rule tables.
- Quoted/escaped command handling behaves identically across TUI and desktop.
- Output clickability does not depend on brittle regex patterns.
- `App.tsx` no longer owns business logic for command grammar.
