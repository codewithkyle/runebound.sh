# Phase 5 Plan

## Objective

Add terminal-style convenience features that improve speed, command recall, and day-to-day usability in the desktop fake CLI.

## In Scope

- Add `clear` command to clear output history/transcript.
- Add command recall with `ArrowUp` and `ArrowDown` when input is empty.
- Cap stored command history to the last 50 commands.
- Add `history` command to inspect recent command entries.
- Add command repeat shortcuts:
  - `!!` to run the previous command
  - `!<n>` to run a specific indexed command from history
- Preserve draft input when navigating command history.
- Persist command history across app restarts.

## Out of Scope

- Shell piping, redirection, or job control.
- Multi-session shared history across machines.
- Full readline-style editing features.

## UX Requirements

- `clear` removes all transcript rows and leaves input focused.
- `ArrowUp` from empty input selects most recent command and walks backward.
- `ArrowDown` walks forward through history and restores pre-navigation draft at the end.
- History navigation should not append output rows by itself.
- `history` prints recent commands in order with stable indexes.
- `!!` executes the last command immediately.
- `!<n>` executes the indexed command if valid, otherwise show a concise error.
- Consecutive duplicate commands should not be added repeatedly to history (recommended).

## Data and Storage

- Keep in-memory history list for active session.
- Persist to app-local file (for example under desktop app data path).
- On startup, load persisted history and clamp to 50 entries.
- On write, keep persisted list capped to 50 entries.

## Implementation Plan

1. Command handling updates
   - Add frontend/local handling for `clear`, `history`, `!!`, and `!<n>` before backend invoke.
   - Keep existing backend invoke path for all other commands.

2. History state model
   - Add typed history store with:
     - command entries
     - navigation cursor index
     - draft buffer while browsing history
   - Enforce max size 50.

3. Keyboard navigation
   - Implement `ArrowUp`/`ArrowDown` handling when input is empty or in history navigation mode.
   - Integrate with existing autocomplete behavior to avoid conflicts.

4. Persistence layer
   - Load history on app startup.
   - Save on every accepted command (or debounced writes).
   - Handle invalid/corrupt persisted history gracefully.

5. Command output behavior
   - `clear` clears transcript only by default.
   - `history` prints a compact numbered list.
   - `!!` and `!<n>` should append the executed command as normal input transcript line.

6. Testing and verification
   - Manual keyboard pass for recall/edit/submit flow.
   - Validate persistence after app restart.
   - Confirm no regressions for autocomplete and `Ctrl+C` behavior.

## Verification Scenarios

- Enter 6 commands, then press `ArrowUp` repeatedly to traverse them backward.
- Press `ArrowDown` to return toward latest, then restore draft input at end.
- Run `clear` and verify transcript is empty.
- Run `history` and verify latest commands are listed with indexes.
- Run `!!` and verify previous command executes.
- Run `!3` and verify command #3 executes.
- Restart app and verify history entries remain available.
- Add more than 50 commands and verify oldest entries are discarded.

## Phase 5 Checklist

- [ ] `clear` command clears transcript output
- [ ] Command history store implemented with max size 50
- [ ] Up/down history recall implemented for empty input
- [ ] Draft input restore implemented after history navigation
- [ ] `history` command implemented
- [ ] `!!` shortcut implemented
- [ ] `!<n>` shortcut implemented
- [ ] Consecutive duplicate history suppression implemented
- [ ] History persistence implemented and loaded on startup
- [ ] Autocomplete integration validated with history navigation
- [ ] Existing `Ctrl+C` clear behavior still works
- [ ] Manual regression pass completed

## Exit Criteria

Phase 5 is complete when users can quickly clear output, recall and rerun recent commands, and keep a stable 50-entry command history that survives app restarts without breaking existing command input and autocomplete behavior.
