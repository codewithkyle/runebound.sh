# Phase 3 Plan

## Objective

Refine the Tauri fake CLI/TUI interface to feel like a native terminal by improving input behavior, focus handling, and visual integration.

## In Scope

- Clear current command input with `Ctrl+C`.
- Ensure command input regains focus when user types while unfocused.
- Remove `$` prompt marker from command input line.
- Remove boxed/pane separation so input feels integrated with output history.
- Remove command suggestion chips.
- Remove Run button and rely on Enter to submit.
- Append `^C` to history when `Ctrl+C` clears non-empty input.

## Out of Scope

- New command features.
- Autocomplete/typeahead logic beyond existing behavior.
- Backend command semantics changes.
- Theming rework beyond CLI-fluidity polish.

## UX Requirements

- Command input appears as part of the transcript, not as a separate boxed footer.
- Output and input share one continuous terminal-like surface.
- `Enter` submits command.
- `Ctrl+C` with non-empty input:
  - clears the input,
  - keeps focus on input,
  - appends `^C` to history.
- Typing any printable key while focus is elsewhere should move focus to input and continue typing naturally.

## Implementation Plan

1. Input behavior updates
   - Add key handling for `Ctrl+C` clear behavior.
   - Add global keydown handler to restore focus to input when typing outside input controls.
   - Keep input focused after submit and after command completion.

2. Layout and styling updates
   - Remove header/input panel borders and segmented boxes.
   - Merge command input presentation into transcript flow.
   - Keep Gruvbox dark tokens while reducing visual chrome.

3. UI cleanup
   - Remove `$` symbol from command row.
   - Remove command suggestion chips.
   - Remove Run button.

4. Transcript behavior
   - On submit, append command line and then output/error lines.
   - On `Ctrl+C` clear, append `^C` as informational transcript line.

5. Verification
   - Confirm all interactions via manual keyboard test flow.
   - Ensure command execution still round-trips to backend invoke path.

## Phase 3 Checklist

- [ ] `Ctrl+C` clears non-empty input
- [ ] `Ctrl+C` appends `^C` line to history
- [ ] Input auto-focuses when typing while unfocused
- [ ] `$` prompt marker removed
- [ ] Suggestion chips removed
- [ ] Run button removed
- [ ] Input visually integrated into output/transcript area
- [ ] Enter submit behavior preserved
- [ ] Gruvbox dark theme retained after layout simplification
- [ ] Manual keyboard interaction pass completed

## Exit Criteria

Phase 3 is complete when the desktop app feels terminal-native for core interaction: command entry is always keyboard-first, `Ctrl+C` behaves like a clear action, and the interface reads as a fluid transcript rather than boxed UI panels.
