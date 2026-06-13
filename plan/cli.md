# CLI Behavior Plan

## Primary UX Rule

- No global command prefix is required.
- Users type direct commands like `npc` and `npc create`.

## Autocomplete and Typeahead

### Trigger

- Typing `npc` then pressing `Tab` opens subcommand suggestions.

### Subcommand suggestions

- `create`
- `list`
- `show`
- `edit`
- `refs`
- `delete`

### Completion behavior

- `npc c` + `Tab` -> `npc create`
- If multiple matches exist, cycle or list based on current UI mode.
- Suggestions should remain visible while the user refines input.

### Contextual suggestions

- After `npc create `, suggest recent references such as `@vault/gods/mu laa.md`.
- For `npc show` and `npc edit`, suggest known NPC ids/slugs.

## Command Parsing

- Commands are split by first token (`npc`) and subcommand token.
- Quoted strings are preserved as one argument.
- Inline references with spaces in file names must be supported.

## Help and Errors

- Entering only `npc` shows concise inline help plus subcommand list.
- Unknown subcommands return a short error and closest suggestion.
- Validation errors should tell the user exactly what to fix.

## Display Conventions

- Keep output compact and scannable.
- Show status line for long operations (indexing, generation, save).
- On success, show saved file path and entity id.

## Safety and Reliability

- Never overwrite files without explicit intent.
- Confirm before destructive operations if behavior changes in future.
- Maintain stable output formatting for predictable testing.
