# Phase 6 Plan

## Objective

Add limited mouse interaction by rendering valid commands in transcript output as clickable command links. Clicking a command executes it immediately.

## In Scope

- Render valid command strings in output as clickable elements.
- Clicked command executes exactly as if typed and submitted.
- Use simple link styling:
  - Gruvbox blue text (`#458588`)
  - underlined
- Keep existing keyboard-first flow intact.

## Out of Scope

- Rich mouse-based UI controls beyond command links.
- Right-click context menus.
- Drag/drop interactions.
- Arbitrary text parsing beyond command detection rules defined in this phase.

## UX Requirements

- Any displayed valid command should be clickable.
- Click target must be visually obvious but minimal (blue + underline).
- Hover state can be subtle (lighter blue or slightly brighter underline).
- Clicking a command:
  1. appends it to transcript as normal input line (`> command`)
  2. executes it through existing command pipeline
  3. preserves history behavior (including cap and dedupe rules)
- Invalid or unknown command-like text should not be clickable.
- Mouse support must not interfere with keyboard behavior (`Tab`, arrows, Enter, `Ctrl+C`).

## Command Detection Rules (v1)

- Command text is considered clickable only if it resolves to a valid command structure.
- Valid clickable commands should include:
  - `status`
  - `help`
  - `config <subcommand>`
  - `npc <subcommand>`
  - `clear`
  - `clear --history`
  - `history`
  - `history <number>`
  - `history clear`
  - `!!`
  - `!<n>`
- Detection source should be centralized and reused (same registry/patterns used by autocomplete/help where possible).

## Implementation Plan

1. Centralize command recognition
   - Add `isValidCommandLike(input: string): boolean` utility.
   - Reuse existing command metadata and built-in command parsing rules.

2. Transcript rendering changes
   - Store output rows as tokenized fragments (plain text + clickable command spans), or parse on render.
   - Render recognized commands as clickable elements.

3. Click execution wiring
   - Add `executeDisplayedCommand(command: string)` handler.
   - Route through same execution path as submitted input (including built-ins and backend invoke).
   - Ensure history and transcript behavior remain consistent.

4. Styling
   - Add command-link style class:
     - color `#458588`
     - `text-decoration: underline`
   - Keep typography/line-height uniform with current system.

5. Safety and anti-noise
   - Prevent accidental double execution (debounce/click lock while running if needed).
   - Ensure only exact recognized command spans are clickable.

6. Verification
   - Manual click pass across history/output.
   - Confirm no regressions in autocomplete, history navigation, and `Ctrl+C` clear.

## Verification Scenarios

- Output contains `config show` -> rendered clickable -> click executes `config show`.
- Output contains plain sentence with non-command words -> not clickable.
- Output contains `history clear` -> clickable and executes alias behavior.
- Clicked command appears in transcript as normal `> ...` entry.
- Rapid click during running command does not break state.
- Keyboard interactions still function unchanged.

## Phase 6 Checklist

- [x] Command detection utility implemented and centralized
- [x] Clickable command rendering implemented in transcript
- [x] Gruvbox blue + underline style applied to clickable commands
- [x] Click handler executes command through shared execution path
- [x] Built-in commands (`clear`, `history`, `!!`, `!<n>`) supported via click
- [x] Invalid/non-command text remains non-clickable
- [x] No regression in keyboard controls (`Tab`, arrows, Enter, `Ctrl+C`)
- [ ] Manual click interaction pass completed
- [x] `make build` passes

## Exit Criteria

Phase 6 is complete when valid commands visible in transcript output are clearly clickable and execute reliably on click, while preserving terminal-like keyboard behavior and visual consistency.
