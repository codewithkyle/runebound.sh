# CLI Behavior Plan

## Primary UX Rules

- No global command prefix is required.
- Users type direct commands (for example `status`, `config show`, `start setup`).
- All non-input output is rendered through structured output documents (`OutputDoc`) rather than ad hoc line styling.

## Current Command Surface (MVP)

- `status`
- `config show`
- `config test`
- `help`
- `start setup`
- `setup help`
- `history`, `history <n>`, `history clear`
- `clear`, `clear --history`
- `exit`

## Autocomplete and Typeahead

- Command metadata is sourced from `core/src/command_manifest.rs`.
- Root and subcommand suggestions are manifest-driven.
- `Tab` completes current suggestion.
- Suggestions remain visible while refining input.

## Parsing and Input Rules

- Parsing authority is backend-first (`core/src/command_parse.rs`).
- Quoted strings are preserved as one argument.
- Markdown-wrapped command input is normalized before parse/execute:
  - `` `help` `` -> `help`
  - `` `config show` `` -> `config show`

## Help, Clickability, and UX Guarantees

- All commands shown as actionable in output/help/history must be clickable and executable.
- If a root command requires a subcommand, clicking the root should run its canonical help command.
- Keyboard UX guarantees must remain stable unless intentionally changed:
  - `Enter` submit
  - `Tab` autocomplete
  - `ArrowUp`/`ArrowDown` history recall
  - `Ctrl+C` input clear

## Output Conventions

- Keep output compact and scannable.
- Prefer explicit command references over sentence-guessing.
- For long operations, show spinner/status blocks.
- Keep command/help copy stable to reduce regressions.

## Maintenance Rules for Future Changes

- Any command add/remove/change must update all of:
  - Clap command definitions
  - command manifest (autocomplete + help metadata)
  - output/help examples and clickable refs
- Validate command changes with `make build` before closing work.
