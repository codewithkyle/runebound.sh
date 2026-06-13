# Phase 2 Plan

## Objective

Add a Tauri desktop shell that presents a fake CLI/TUI interface with:

- command input pinned to bottom,
- scrollable output/history area,
- Gruvbox dark visual theme.

## In Scope

- Add Tauri app scaffold and run desktop app locally.
- Build fake terminal-style UI in Tauri frontend.
- Use SolidJS + Vite + Tailwind CSS for frontend implementation.
- Route command execution to Rust backend command dispatcher.
- Persist and render command history in a scrollable pane.
- Keep command input fixed at bottom and always focusable.
- Apply Gruvbox dark theming tokens globally.
- Support existing `config` commands from Phase 1 through UI.

## Out of Scope

- NPC generation implementation details.
- Advanced autocomplete behavior beyond basic command hints.
- Multi-pane campaign dashboards.
- Production packaging/signing.

## Architecture

- Reuse existing Rust logic (`dnd-core`) in Tauri backend commands.
- Add Tauri desktop app scaffold.
- Frontend stack:
  - SolidJS
  - Vite
  - Tailwind CSS
- Frontend renders:
  - `OutputPanel` (scrollable history/log)
  - `CommandBar` (bottom-locked input)
  - optional `SuggestionStrip` (simple command hints)
- Command flow:
  1. User submits text command.
  2. Frontend appends `> command` to history.
  3. Frontend calls Tauri invoke endpoint.
  4. Backend parses and executes command using shared dispatch path.
  5. Frontend appends structured result (success/error/output).
  6. Output panel auto-scrolls (unless user manually scrolled up).

## UI/UX Requirements

- Bottom command input always visible and keyboard-first.
- Enter submits command.
- Output panel scrollable with mouse and keyboard.
- Distinct styles for:
  - user input lines,
  - normal output,
  - warnings,
  - errors.
- Empty state shows quick examples:
  - `config init`
  - `config show`
  - `config doctor`

## Gruvbox Dark Theme Spec

- Background: `#282828`
- Surface: `#32302f`
- Surface-2: `#3c3836`
- Text primary: `#ebdbb2`
- Text muted: `#a89984`
- Accent: `#d79921`
- Success: `#98971a`
- Warning: `#d79921`
- Error: `#cc241d`
- Info: `#458588`
- Border: `#504945`
- Use monospaced font stack suitable for terminal feel.

## Backend Integration Tasks

- Expose Tauri command `run_command(input: String) -> CommandResponse`.
- `CommandResponse` fields:
  - `ok: bool`
  - `output: String`
  - `error: Option<String>`
  - `exit_code: i32`
- Move or encapsulate command dispatch in shared Rust module so both CLI and Tauri can reuse it.
- Ensure config gating behavior still applies in desktop mode.

## Frontend Tasks

- Create layout with full-height container:
  - top: output/history (`overflow-y: auto`)
  - bottom: sticky command bar
- Add command history state model with typed entries.
- Add submit lifecycle states (idle/running/done/error).
- Add simple hint row for known commands (`config init/show/test/doctor`, `status`).
- Ensure responsive behavior for smaller window sizes.

## Verification

- Tauri app launches and renders fake terminal UI.
- Bottom input remains visible while history grows.
- History pane scrolls with many command outputs.
- Commands execute and output is rendered correctly.
- `config` command flows work in Tauri as in CLI.
- Theme colors match Gruvbox dark palette.

## Phase 2 Checklist

- [x] Tauri app scaffolded and runs locally
- [x] SolidJS + Vite + Tailwind scaffolded for frontend
- [x] Shared command-dispatch path reusable by CLI and Tauri
- [x] `run_command` Tauri backend command implemented
- [x] Full-height fake terminal layout implemented
- [x] Bottom-locked command input implemented
- [x] Scrollable output/history panel implemented
- [x] Command result rendering (ok/warn/error) implemented
- [x] Basic command hint strip implemented
- [x] Gruvbox dark theme tokens added and applied
- [x] Keyboard-first behavior validated
- [x] Existing `config` commands verified through UI
- [x] Phase 2 docs updated with any scope adjustments
