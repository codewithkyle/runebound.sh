# Phase 4 Plan

## Objective

Implement typeahead and autocomplete in the desktop fake CLI for the initial command set so command discovery is fast and keyboard-native.

## In Scope

- Typeahead suggestions while typing in command input.
- Tab completion for commands and subcommands.
- Keyboard navigation of suggestions.
- Context-aware suggestions for first token and subcommands.
- Integration with existing command surface:
  - `status`
  - `config init`
  - `config show`
  - `config test`
  - `config doctor`
  - `help`
- Baseline support for future `npc` command group in suggestion model.

## Out of Scope

- LLM-backed or fuzzy natural-language command interpretation.
- File path autocomplete for vault references.
- Rich command docs panel and inline manuals.
- New backend command features.

## UX Requirements

- Suggestions appear immediately as user types.
- `Tab` completes the highlighted suggestion.
- `Shift+Tab` cycles backwards through suggestions.
- `ArrowUp` and `ArrowDown` move through suggestion list when list is open.
- `Enter` submits current command (or selected suggestion if not yet applied; exact behavior to be defined).
- `Esc` closes suggestion list without clearing input.
- Suggestions should never steal focus from the input.

## Command Suggestion Model

- Build a normalized suggestion tree for:
  - top-level commands
  - subcommands per top-level command
  - optional flag hints for selected command
- Each suggestion item should include:
  - display label
  - completion value
  - kind (`command`, `subcommand`, `flag`)
  - short description (optional in v1 UI)

## Implementation Plan

1. Shared command metadata source
   - Define a single command metadata registry in Rust (`dnd-core`) or frontend constants generated from backend.
   - Ensure metadata aligns with clap command grammar.

2. Frontend suggestion engine
   - Tokenize current input by shell-like rules.
   - Resolve context (first token, subcommand position, flag context).
   - Return filtered suggestions by prefix.

3. Input interaction layer
   - Add keyboard handlers for `Tab`, `Shift+Tab`, arrows, and `Esc`.
   - Apply completion without breaking existing Enter submit behavior.
   - Keep existing `Ctrl+C` clear behavior intact.

4. Suggestion rendering
   - Render lightweight suggestion list anchored near input.
   - Highlight active selection.
   - Keep style minimal and consistent with current terminal aesthetic.

5. Validation and polish
   - Verify autocomplete for all in-scope commands.
   - Ensure no regressions in command submission flow.
   - Ensure suggestions stay performant with rapid typing.

## Verification Scenarios

- Typing `c` suggests `config`.
- Typing `config ` suggests `init`, `show`, `test`, `doctor`.
- Typing `config d` then `Tab` completes to `config doctor`.
- Typing `sta` then `Tab` completes to `status`.
- `Esc` closes suggestions and leaves input unchanged.
- `Enter` still runs the final command and appends output to history.

## Phase 4 Checklist

- [x] Command metadata source created and documented
- [x] Suggestion engine implemented for top-level commands
- [x] Suggestion engine implemented for `config` subcommands
- [x] Suggestion engine supports prefix filtering
- [x] Suggestion UI rendered in desktop app
- [x] Active suggestion highlight implemented
- [x] `Tab` apply completion implemented
- [x] `Shift+Tab` reverse cycling implemented
- [x] Arrow key suggestion navigation implemented
- [x] `Esc` closes suggestions implemented
- [x] Existing input behaviors (`Enter`, `Ctrl+C`, focus capture) still pass
- [x] Manual keyboard interaction pass completed

## Exit Criteria

Phase 4 is complete when users can discover and complete the initial command set quickly using only keyboard input, with a reliable and terminal-like autocomplete workflow.
