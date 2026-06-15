# Calendar Feature Roadmap

> Follow `docs/architecture.md`, `docs/cli.md`, and `docs/feature-development.md` for layer boundaries, manifest hygiene, and verification. Each phase below assumes we keep the single source of truth for command metadata in `command-specs/` and route behavior through desktop handlers/services, never through `router.rs` or `main.rs`.

## Phase 1 — Calendar Import (donjon.bin.sh)

### Goals
- Let users import JSON exports from https://donjon.bin.sh/fantasy/calendar/.
- Normalize the JSON into a canonical TOML file stored under the existing config directory (reuse `ConfigPaths`).
- Reset the active calendar state to year `0`, first month/day, midnight.

### Workstream
1. **Modeling (`dnd_core`):**
   - Add a `calendar` module with `CalendarDefinition`, `MonthSpec`, `StoredCalendar`, and `CalendarState` structs (serde-enabled, TOML-friendly).
   - Include validation helpers (e.g., ensure `month_len` matches `months`, positive day counts, consistent week length).
2. **Persistence:**
   - Extend `config::ConfigPaths` (or add a peer helper) with `calendar_toml` path under `~/.config/runebound.sh/`.
   - Implement `load_calendar()` / `save_calendar()` returning `Result<Option<StoredCalendar>>` with informative errors.
3. **Importer:**
   - Parse the donjon JSON shape (fields: `year_len`, `months`, `month_len`, `week_len`, `weekdays`, `lunar_cyc`, `lunar_shf`, `first_day`).
   - Map moons into a simple vector; stash unsupported keys in a `notes` map for forward compatibility.
   - Initialize `CalendarState` defaults (year `0`, month index `0`, day `1`, `hour_24 = 0`, `minute = 0`).
   - Unit tests: round-trip JSON → TOML, invalid month length errors, missing field handling.
4. **Command Surface (`command-specs` + desktop handler):**
   - Add `calendar` command with `import` subcommand in manifest (execution target `Desktop`).
   - Create `desktop/src-tauri/src/commands/calendar_commands.rs` handling:
     - `calendar import <path>` to import a provided JSON file.
     - `calendar import` with no path to open a `tauri::api::dialog::FileDialogBuilder` filtered to `.json`.
     - Overwrite confirmation message in output (explicitly note previous state replaced).
   - Register handler in `commands/mod.rs`.
   - Use `command_ref("calendar import")` in success/help output per `docs/cli.md`.
5. **Docs & Verification:**
   - Document JSON expectations, storage location, and overwrite semantics in `docs/cli.md`.
   - Add architecture note (data now lives in config dir) to `docs/architecture.md` if needed.
   - Tests: unit (`cargo test` in `core`), manual command run via desktop CLI.

## Phase 2 — Date Commands

### Goals
- Provide `date` command family to inspect and mutate year/month/day directly.
- Enforce validation (year ≥ 0, month must exist, day within selected month).
- Keep behavior desktop-only while reusing the core calendar module.

### Workstream
1. **Manifest:**
   - Add `date` command with `set` subcommand options in `command-specs`. Examples should cover `date`, `date set year 5`, `date set month Emberwane`, `date set day 14` per docs help style.
2. **Handler (`desktop/src-tauri/src/commands/date_commands.rs`):**
   - `date` (no args): load calendar, error with actionable guidance if none (`command_ref("calendar import")`).
   - `date set year <number>`: parse `i32`, guard against negatives, update state, persist, emit formatted date string and `output_doc` showing weekday + ordinal.
   - `date set month <name>`: case-insensitive match against definition; reject unknown names with list of valid entries.
   - `date set day <number>`: ensure `1..=month_length` after applying month change.
   - `date set time <HH:MM> [AM|PM]`: parse 12-hour inputs with optional suffix, default suffix to AM when omitted, accept 24-hour tokens (e.g., `13:30`) and convert to stored 24-hour representation before formatting output.
   - Centralize formatting in `dnd_core::calendar::format_date(&StoredCalendar)` returning `14th of Emberwane 10:30 AM (Moonday)` to keep CLI doc-compliant.
3. **Services/Utilities:**
   - Provide `CalendarState::set_year/month/day` helpers that return `Result<()>` so handler stays orchestration-only (`docs/architecture.md`).
4. **Error UX:**
   - Mirror CLI guidelines by providing usage hints via text + `command_ref` for each invalid subcommand.
5. **Testing & Docs:**
   - Add unit tests for setters and formatting (ordinal suffix, AM/PM conversion, weekday derivation using `first_day` and `week_len`).
   - Update `docs/cli.md` “Commands” table with `date` usage, mention that it requires an imported calendar.

## Phase 3 — Time Commands

### Goals
- Support relative time adjustments (minutes, hours, days, weeks, years) with standalone `+/-` commands (e.g., `+5h`, `-2d`), including rollover across months/years.
- Persist changes immediately and echo the updated formatted date after each adjustment.

### Workstream
1. **Delta Parsing (core calendar module):**
   - Implement `CalendarDelta::from_str("+3d")` supporting units `m`, `h`, `d`, `w`, `y`, optional `+/-`, multi-digit magnitudes, and case-insensitive units.
   - Expose `apply_delta(&mut CalendarState, &CalendarDefinition, CalendarDelta)` that uses minute-level arithmetic, converts weeks via `week_len`, years via cumulative month lengths, and handles underflow (no negative years; clamp at year 0, day 1 if subtraction overshoots).
2. **Handler Additions:**
   - Add standalone handlers in `time_delta_commands.rs` so commands like `+5h`, `-2d`, `+1w`, `+1y`, `+30m` call the delta helper.
   - Reject mixed/multi-token expressions per CLI simplicity (one delta per invocation). Provide explicit usage errors for invalid tokens.
   - Always print the new formatted date plus a short delta summary (e.g., “Added 5 hours → 14th of Emberwane 3:30 PM”).
3. **Time-of-Day Formatting:**
   - Ensure `format_date` maps 0 → `12:00 AM`, uses leading zeros for minutes, and appends `AM/PM` as requested.
4. **Undo/History Considerations:**
   - Push command text to history the same way other commands do; no additional state resets required.
5. **Docs & Tests:**
   - Extend `docs/cli.md` with a “Relative Time” subsection documenting syntax, supported units, and subtraction behavior.
   - Add unit tests for each delta unit, boundary rollover (minute → hour/day, day → next month, month → next year), and subtraction clamping at year 0.
   - If weekday/week count exposure becomes necessary, add acceptance criteria before implementation (future stretch).

## Cross-Phase Quality Gates
- **Manifest compliance:** Every new command/subcommand added in the manifest first, per `docs/cli.md`.
- **Structured output:** Provide `output_doc` + `command_ref` for guidance/next steps.
- **Tests:** Run `cargo test` for affected crates and add targeted unit tests (import parsing, setters, delta math).
- **Documentation:** Update `docs/cli.md` (command UX), `docs/architecture.md` (storage location, module overview), and optionally `docs/render.md` if new output nodes are introduced.
- **Manual verification:** After implementation, exercise the flows inside the desktop UI: import, view date, set fields, run +/- adjustments, confirm history and autocomplete behave per `docs/feature-development.md`.

## Phase 4 — Lunar Phases (Bonus)

### Goals
- Provide a standalone `moon` command that reports the current phase for each configured moon.
- Reuse `lunar_cyc` (cycle lengths) and `lunar_shf` (phase offsets) captured during calendar import.

### Workstream
1. **Core Helpers:** add `moon_phase_info(&StoredCalendar)` plus supporting types (`MoonPhaseKind`, `MoonPhaseInfo`, `total_days_since_epoch`) that compute each moon’s age and map it to standard lunar phases.
2. **Handler:** create `desktop/src-tauri/src/commands/moon_commands.rs` with `moon`/`moon help` handling; output each moon’s phase, day-in-cycle, and cycle length; handle missing lunar metadata with actionable errors.
3. **Manifest & Docs:** register the `moon` command in `command-specs`, document usage in `docs/cli.md`, and mention the requirement for lunar data exported from donjon.
4. **Testing:** unit-test the moon phase math for multiple cycles/shifts; manually verify via the desktop CLI after importing a calendar that includes lunar data.
