# CLI Behavior Plan

> **Purpose:** This document defines command UX rules, command implementation boundaries, and verification steps for the runebound.sh interface.

---

## 1. Primary UX Rules

- No global prefix. Users type direct commands (`status`, `config show`, `create npc`).
- Structured output (`output_doc`) is preferred; plain text (`output`) is compatibility fallback.
- Commands are split by execution target:
  - Core: `status`, `config`, `help`, `exit`, `setup`
  - Desktop: `calendar`, `create`, `npc`, `location`, `faction`, `item`, `load`, `show`, `preview`, `delete`, `undo`, `save`, `reroll`, `cancel`, `clear`, `history`
- Router remains dispatch-only (`desktop/src-tauri/src/router.rs`).

---

## 2. Command Manifest (Single Source of Truth)

**Golden rule:** all command metadata lives in `command-specs/src/lib.rs`.

Each `CommandSpec` declares:

- name and summary
- examples
- subcommands and options
- `requires_subcommand`
- `canonical_help_command`
- execution target
- autocomplete visibility

Manifest data is used by:

- suggestion service (`desktop/src-tauri/src/services/suggestions.rs`)
- help generation in `core/src/command.rs`
- registry metadata (`handler_metadata_for()`)
- frontend clickable-command fallback resolution

If command shape changes without manifest changes, UX consistency breaks.

---

## 3. Parse and Dispatch Contracts

### Parser Authority

- Backend parser in `core/src/command_parse.rs` is canonical.
- Frontend parsing is lightweight and suggestion-oriented only.

### Parse Rules

- Quoted arguments are preserved.
- Markdown-wrapped command input is normalized.
- Aliases resolve at parse time.
- `-h` and `--help` are intentionally rejected in favor of phrase help.

### Dispatch Rules

- Core dispatch: `core/src/command.rs` registry.
- Desktop dispatch: `desktop/src-tauri/src/commands/mod.rs` registry via `router.rs`.
- Unknown desktop roots may fall back to entity resolution for load/show/preview behavior.

---

## 4. Autocomplete and Typeahead

Flow:

1. Frontend invokes `suggest_command_input`
2. Backend suggestion service reads parsed context + manifest
3. Suggests commands, subcommands, entities, location names (`npc travel to`), and `@reference` keys
4. Frontend displays helper text by kind

Key behavior:

- `Tab` completes current suggestion
- suggestions stay live as user edits
- suggestions are filtered by the active editor kind (`EntityKind` from `EditorSession`)
- Global system commands (`save`, `reroll`, `cancel`) must stay visible whenever any draft is active; only hide them when `active_kind` is `None`
- `entity_kind_for_root()` in `services/suggestions.rs` must map every supported root to its `EntityKind` so field completions work
- `build_entity_field_argument_suggestions()` pulls directly from `settable_fields`/`rerollable_fields`; add schema entries before exposing new fields
- Every autocomplete change requires a matching unit test in `services/suggestions.rs` (`cargo test suggestions`)

Current active kinds:

- `None`, `Npc`, `Location`, `Faction`, `Item`

When adding a new entity, update `EntityKind`, schemas, command handlers, and suggestion filters together.

---

## 5. Help and Clickability Guarantees

### Help

- Phrase help is required: `help <command>` and `<command> help`
- Help comes from manifest metadata
- Do not add `-h` or `--help`

### Clickability

- Actionable command text should be clickable.
- Preferred path: backend emits `InlineNode::CommandRef`.
- Fallback path: `markdown.ts` heuristics.

For any new command, ensure at least one explicit `command_ref` path exists in guidance output.

---

## 6. Implementation Guidelines

### Adding a Top-Level Command

1. Add `CommandSpec` in `command-specs/src/lib.rs`
2. Implement command domain handler module under `desktop/src-tauri/src/commands/`
3. Register handler entry in `desktop/src-tauri/src/commands/mod.rs`
4. Add structured output and command refs for usage/help
5. Update suggestions only if command has special argument completion

### Adding a Subcommand

1. Add subcommand metadata to manifest
2. Add parsing/behavior in existing domain command module
3. Add/verify help output and examples
4. Update suggestion logic for field-level completion if needed

### Adding New Command Arguments/Fields

1. Update canonical field mapping functions in command/service modules
2. Keep error messages explicit and include valid fields
3. Update suggestion field lists in `services/suggestions.rs`
4. Verify command refs and usage text stay aligned with behavior

### Entity Command Rules

- Entity roots (`npc`, `location`, `faction`, `item`, future kinds) must delegate to their `EntityDomain` implementations.
- `system save|reroll|cancel` rely on `EditorSession::active_kind`; keep drafts synchronized whenever commands mutate state.
- `entity_commands.rs` (load/show/preview/delete/undo) must hydrate drafts and emit client events using the shared builders in `entities/domains/*`.
- Register every entity domain with `EntityDomainRegistry` in `main.rs` so registry lookups succeed inside command modules.

---

## 7. Maintenance Rules

- Do not add command business logic to `main.rs` or `router.rs`.
- Do not bypass the registry for top-level commands.
- Do not bypass repository/service boundaries in command handlers.
- Keep command output compact, stable, and structured.
- Prefer explicit `command_ref` over parser heuristics.

---

## 8. Verification Checklist

Before merging any CLI or command behavior change:

- [ ] `command-specs/src/lib.rs` updated for all command metadata changes
- [ ] Handler logic implemented in correct command domain module
- [ ] Handler registered in desktop/core registry builder
- [ ] `help <command>` and `<command> help` produce expected content
- [ ] Autocomplete shows command/subcommands/fields correctly
- [ ] Actionable output uses clickable command refs
- [ ] Active draft kind transitions still behave correctly (create/load/save/cancel/reroll)
- [ ] Keyboard invariants still hold (`Enter`, `Tab`, arrows, `Ctrl+C`)
- [ ] `make build` passes

---

## 9. Calendar Import

The `calendar` command imports fantasy calendars from JSON exports produced by [donjon.bin.sh/fantasy/calendar/](https://donjon.bin.sh/fantasy/calendar/).

### Usage

```
calendar import <path>
calendar import path/to/calendar.json
```

### JSON Expectations

The importer expects donjon.bin.sh JSON format with the following fields:

| Field | Type | Description |
|---|---|---|
| `year_len` | integer | Total days in a year |
| `months` | array of strings | Month names |
| `month_len` | object | Mapping of month name to day count |
| `week_len` | integer | Days in a week |
| `weekdays` | array of strings | Weekday names |
| `moons` | array of strings | Moon names |
| `lunar_cyc` | object | Moon cycle lengths (stored in notes) |
| `lunar_shf` | object | Moon shift values (stored in notes) |
| `first_day` | integer | First day of year (weekday index) |

### Storage Location

Imported calendars are normalized to TOML and stored at:

```
~/.config/runebound.sh/calendar.toml
```

### Overwrite Semantics

- Importing a calendar **always replaces** any previously stored calendar.
- The active calendar state is **reset to year 0, first month, day 1, midnight (00:00)** after import.
- Unsupported JSON fields are stashed in a `notes` map for forward compatibility.

---

## 10. Date Commands

The `date` command displays and modifies the currently loaded calendar date.

### Usage

```
date
date set year <number>
date set month <month-name>
date set day <number>
date set time <HH:MM> [AM|PM]
```

### Examples

```
date
date set year 5
date set month Emberwane
date set day 14
date set time 12:15 PM
date set 1:00
date set 13:30
```

### Requirements

- Requires an imported calendar (`calendar import`)
- Year must be ≥ 0
- Month name is case-insensitive; must match an existing month
- Day must be within the valid range for the selected month
- Time defaults to AM when no suffix is provided; 24-hour inputs (e.g., `13:30`) are also supported and converted for display

### Output Format

Current date displays as: "14th of Emberwane 2:30 PM (Moonday)"

The weekday is computed from the calendar's `first_day` offset and `week_len`.

---

## 11. Relative Time Commands

Standalone `+` and `-` commands adjust the current calendar forward or backward.

### Usage

```
+<amount><unit>
-<amount><unit>
```

Where `<unit>` is one of:

- `m` — minutes
- `h` — hours
- `d` — days
- `w` — weeks (uses the calendar's `week_len`)
- `y` — years (uses the calendar's `year_len`)

### Examples

```
+30m
+5h
-2d
-1w
```

### Rules

- Requires an imported calendar (`calendar import`).
- Amount must be a positive integer (e.g., `+5h`).
- Only one delta token is accepted per command.
- Subtractions clamp at the campaign start (year 0, first month/day).
- Each command persists immediately and echoes the updated formatted date.

---

## 12. Moon Command

Display the current phase for each moon defined in the active calendar.

### Usage

```
moon
```

### Requirements

- Requires an imported calendar that includes lunar data (`lunar_cyc`, optional `lunar_shf`).

### Output

- Lists each moon, its phase (new/full/etc.), and its day within the cycle.

---

## 13. Publish Command

Generate a reader-friendly markdown file for any saved entity and write it to the configured Obsidian vault.

### Usage

```
publish
publish <name-or-slug>
publish help
```

### Behavior

- If you have an active draft loaded in the editor, `publish` with no arguments targets that draft.
- Pulls the canonical TOML record from `~/.config/runebound.sh/entities/...`.
- Renders entity data into a structured markdown layout (metadata table, sections, bullet lists).
- Writes to the entity's `vault_path` (e.g., `npcs/lirael.md`).
- If the file already exists you must confirm the overwrite; declining leaves the file untouched.
- Publishing is one-way—manual edits inside Obsidian are not synced back. Re-run `publish` after making changes via commands.
- Startup sync mirrors the canonical TOML store into the SQLite database; Obsidian files are treated purely as render targets.

---

## 14. Related Docs

- `docs/architecture.md` for module boundaries and extension strategy
- `docs/render.md` for output and renderer contracts
- `docs/feature-development.md` for end-to-end feature build playbooks

---

*Last updated: 2026-06-15*  
*Update this doc whenever command UX contracts or command flow rules change.*
