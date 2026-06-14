# CLI Behavior Plan

> **Purpose:** This document defines the UX rules, command conventions, and maintenance rules for the runebound.sh command interface. It is the companion to `architecture.md` (module structure) and `render.md` (output pipeline). Future agents MUST read all three before changing commands.

---

## 1. Primary UX Rules

- **No global command prefix.** Users type direct commands (`status`, `config show`, `create npc`).
- **All command output is rendered through structured output documents (`OutputDoc`)** where possible. Plain text (`output`) is a compatibility fallback only.
- **Commands are either `Core` or `Desktop`.**
  - `Core` commands (`status`, `config`, `help`, `exit`, `setup`) execute in `core/src/command.rs` via the core `HandlerRegistry`.
  - `Desktop` commands (`create`, `npc`, `location`, `faction`, `load`, `show`, `delete`, `undo`, `save`, `reroll`, `cancel`, `clear`, `history`) execute in `desktop/src-tauri/src/commands/*.rs` via the desktop `HandlerRegistry`.
- **The router is dispatch-only.** `desktop/src-tauri/src/router.rs` is 51 lines. It resolves the first token against the registry and calls the handler. It contains zero business logic.

---

## 2. Command Manifest (Single Source of Truth)

> **Golden rule:** The command manifest is the single source of truth for all command metadata.

- **Location:** `command-specs/src/lib.rs`
- **Contents:** `CommandManifest` with `commands` (list of `CommandSpec`) and `aliases` (list of `CommandAlias`)
- **Each `CommandSpec` defines:** name, summary, examples, subcommands, options, `requires_subcommand`, `canonical_help_command`, `execution` (`Core` or `Desktop`), and `show_in_autocomplete`

### Why This Matters

The manifest is read by:
- **Autocomplete engine** (`suggest_command_input` in `main.rs`) — filters commands by `show_in_autocomplete`
- **Help text generator** (`render_command_help`, `render_subcommand_help` in `core/src/command.rs`) — builds help from spec metadata
- **Command registry builders** (`build_core_handler_registry`, `build_desktop_handler_registry`) — metadata for `HandlerEntry`
- **Frontend parser** (`markdown.ts`) — resolves command-like text against the manifest

**Rule:** Adding, removing, or changing a command name, subcommand, or alias without updating the manifest breaks autocomplete and help.

---

## 3. Autocomplete and Typeahead

### How It Works

1. User types in the input field.
2. Frontend calls `suggest_command_input(input)` via Tauri invoke.
3. Backend parses the input with `core/src/command_parse.rs`.
4. Backend reads the manifest and suggests:
   - Root commands matching the current token
   - Subcommands matching the current token (if a root is selected)
   - Entity names (if the context is `load`, `show`, `delete`, `preview`)
   - Location names (if the context is `npc travel to`)
   - Vault reference keys (if the user is typing `@something`)
5. Frontend renders suggestions with `SuggestionHelperText` (Command, Npc, Location, Faction, Reference).

### Key Behaviors

- `Tab` completes the current suggestion.
- Suggestions remain visible while refining input.
- Suggestions are filtered by `EditorMode`:
  - `npc` subcommands are hidden unless `EditorMode::Npc` is active
  - `location` subcommands are hidden unless `EditorMode::Location` is active
  - `faction` subcommands are hidden unless `EditorMode::Faction` is active
  - `reroll` is hidden unless any editor mode is active
  - `cancel` is hidden unless `EditorMode::None`

---

## 4. Parsing and Input Rules

### Authority

- **Parsing authority is backend-first.** `core/src/command_parse.rs` is the canonical parser.
- The frontend does its own lightweight parsing for autocomplete, but the backend is the source of truth.

### Rules

- **Quoted strings are preserved as one argument.**
- **Markdown-wrapped command input is normalized before parse/execute:**
  - `` `help` `` → `help`
  - `` `config show` `` → `config show`
- **Command aliases are resolved at parse time.** `core/src/command_parse.rs` normalizes alias tokens against the manifest.
- **`-h` and `--help` are rejected.** The parser explicitly rejects `--help` and `-h` with a message telling the user to use `help <command>` or `<command> help`.

---

## 5. Command Dispatch Architecture

### The Registry Pattern

Both core and desktop use the same pattern from `command-handler`:

```
HandlerRegistry<Bridge>
    → HandlerEntry<Bridge> (name, metadata, handler)
        → HandlerBridge::invoke(invocation)
            → executes command logic
```

### Core Dispatch

- `core/src/command.rs` → `execute_line_with_session()` → `execute_dispatched()`
- `core_handler_registry()` → `build_core_handler_registry()` → registers `status`, `config`, `help`, `exit`, `setup`

### Desktop Dispatch

- `desktop/src-tauri/src/main.rs` → `run_command()` → `router::dispatch_desktop_command()`
- `desktop_handler_registry()` → `build_desktop_handler_registry()` → registers `exit`, `clear`, `history`, `create`, `npc`, `location`, `faction`, `load`, `show`, `preview`, `delete`, `undo`, `save`, `reroll`, `cancel`
- If no registry match, `router.rs` falls back to entity resolution (name/slug search) for `load`/`show`/`preview` behavior.

### Adding a New Command

See `architecture.md` §8 for the full step-by-step guide. In short:

1. Add `CommandSpec` to `command-specs/src/lib.rs`
2. Create `commands/<domain>_commands.rs` (or extend existing)
3. Register in `commands/mod.rs` via `*_handler_entry()`
4. **No router changes needed for new top-level commands.**

---

## 6. Help, Clickability, and UX Guarantees

### Help System

- **Phrase-based help only.** `help`, `help <command>`, `<command> help`, `<subcommand> help`
- **No `-h`/`--help` support.** This is enforced by the parser.
- Help text is generated from the manifest, not hardcoded strings.

### Clickability

- All commands shown as actionable in output/help/history must be clickable and executable.
- **Best path:** Backend emits `InlineNode::CommandRef` in `output_doc`.
- **Fallback path:** `markdown.ts` detects command-like text and resolves against the manifest.
- If a root command requires a subcommand, clicking the root should run its `canonical_help_command`.

### Keyboard UX Guarantees

These must remain stable unless intentionally changed:
- `Enter` — submit command
- `Tab` — autocomplete current suggestion
- `ArrowUp` / `ArrowDown` — command history recall
- `Ctrl+C` — clear input

---

## 7. Output Conventions

- Keep output compact and scannable.
- **Prefer explicit `command_ref` over sentence-guessing.** The parser fallback is unreliable.
- For long operations, show `spinner` blocks.
- Keep command/help copy stable to reduce regressions.
- **Errors should use `status(error)` semantics** where possible.
- **Info/guidance should use `status(info)` semantics** where possible.

---

## 8. Editor Mode Behavior

The desktop has an `EditorMode` (`None`, `Npc`, `Location`, `Faction`) that controls which commands are available:

| Mode | Active Commands | Hidden Commands |
|---|---|---|
| `None` | `create`, `load`, `show`, `preview`, `delete`, `undo`, `clear`, `history`, `help`, `status`, `config`, `exit` | `npc`, `location`, `faction`, `reroll`, `save`, `cancel` |
| `Npc` | `npc`, `reroll`, `save`, `cancel`, `create` | `location`, `faction` |
| `Location` | `location`, `reroll`, `save`, `cancel`, `create` | `npc`, `faction` |
| `Faction` | `faction`, `reroll`, `save`, `cancel`, `create` | `npc`, `location` |

When a draft is loaded or created, the editor mode switches to that entity type. When a draft is saved or cancelled, the mode switches to the next available draft or `None`.

---

## 9. Maintenance Rules for Future Changes

### Adding a New Command

Any command add/remove/change must update all of:

1. **`command-specs/src/lib.rs`** — Add/remove `CommandSpec` and `CommandAlias` entries
2. **`commands/*.rs`** (desktop) or **`core/src/command.rs`** (core) — Implement handler logic
3. **`commands/mod.rs`** (desktop) or **`core/src/command.rs`** (core) — Register in `build_*_handler_registry()`
4. **Help output** — Manifest-driven; verify with `help <command>` and `<command> help`
5. **Clickable command refs** — Ensure `output_doc` uses `command_ref` for actionable text

### Rules

- **Phrase help is required.** `help <command>` and `<command> help` must both work.
- **`-h`/`--help` should not be introduced.** The parser actively rejects them.
- **Validate with `make build` before closing work.**
- **Do not add command logic to `router.rs` or `main.rs`.** Use `commands/*.rs` modules.
- **Do not bypass the registry.** All top-level commands must be registered in a `HandlerRegistry`.

---

## 10. Verification Checklist

Before merging any command change:

- [ ] `command-specs/src/lib.rs` updated with correct `CommandSpec`
- [ ] Handler registered in `build_*_handler_registry()`
- [ ] `help <command>` works and shows correct subcommands/examples
- [ ] `<command> help` works and shows the same content
- [ ] Autocomplete suggests the command and its subcommands
- [ ] Clickable command refs in output execute correctly
- [ ] `make build` passes
- [ ] Keyboard UX (`Enter`, `Tab`, arrows, `Ctrl+C`) still works
- [ ] No new command logic added to `router.rs` or `main.rs`

---

*Last updated: 2026-06-14*  
*If this document is outdated, update it before changing commands.*
