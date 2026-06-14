# NPC Command Plan

## Scope

Initial command surface focuses only on NPC workflows.

## Subcommands

### `npc create`

- Generate a new NPC from prompt text.
- Accept optional references to vault files.
- Save result to `vault/npcs/<slug>.md`.
- Add or refresh SQLite index entry.

Example forms:

- `npc create "priest of @vault/gods/mu laa.md"`
- `npc create --name "Father Elen" "stern temple caretaker with a secret"`
- `npc create --ref "vault/gods/mu laa.md" "traveling cleric"`

### `npc list`

- List available NPCs from active index.
- Default sort: recently updated descending.
- Include id/slug, name, updated timestamp.

### `npc show`

- Display one NPC by id, slug, or exact name match.
- Render key metadata and markdown sections.

### `npc edit`

- Regenerate or update targeted sections.
- Preserve non-edited sections unless user requests full regenerate.
- Update markdown file and index.

### `npc refs`

- View refs attached to an NPC.
- Add and remove references used for future generations.

### `npc delete`

- Soft delete only in v1.
- Global command form is `delete <npc-or-location-name>`.
- NPC files move to `vault/.trash/npcs/` and locations move to `vault/.trash/locations/`.
- `undo` restores the most recently deleted entity (LIFO).

## Save Behavior

- Saving replaces only the fenced ` ```runebound ` metadata block.
- Any non-runebound content in the file is preserved verbatim (notes, embeds, headings, custom text).
- NPC and location markdown filenames use readable proper names (for clean `[[Wiki Links]]`), with numeric suffixes only when needed for collision handling.

## Input and Output Rules

- Input prompt can include `@vault/...` references inline.
- Final file format is a fenced metadata block using ```runebound with TOML content.
- `type` is always `npc` for this command group.

## Validation

- Reject missing prompt for `npc create`.
- Validate that referenced files exist inside configured vault root.
- Prevent path traversal outside vault root.

## Future Commands (not in MVP)

- `npc search`
- `npc rename`
- `npc template`
