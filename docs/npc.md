# NPC Command Plan

## Scope

Desktop command surface supports guided NPC and location editing.

## Active Commands

### `create npc [prompt...]`

- Start guided NPC creation.
- Optional prompt text shapes generated draft content.
- Draft stays in editor mode until saved or cancelled.

### `load <npc-or-location-name>`

- Load existing NPC or location into editor mode.

### `npc` editor commands

- `npc show`
- `npc rename <name>`
- `npc set <field> <value>`
- `npc travel to <location>`
- `npc reroll <field> [prompt]`
- `npc save`
- `npc cancel`

### `location` editor commands

- `location show`
- `location rename <name>`
- `location save`
- `location cancel`

### Global context commands

- `save`
- `reroll` (NPC editor only)
- `cancel`

### Delete / restore

- Global command form is `delete <npc-or-location-name>`.
- NPC files move to `vault/.trash/npcs/` and locations move to `vault/.trash/locations/`.
- `undo` restores the most recently deleted entity (LIFO).

## Save Behavior

- Saving replaces only the fenced ` ```runebound ` metadata block.
- Any non-runebound content in the file is preserved verbatim (notes, embeds, headings, custom text).
- NPC and location markdown filenames use readable proper names (for clean `[[Wiki Links]]`), with numeric suffixes only when needed for collision handling.
- New and renamed entities get readable filenames like `npcs/Father Elen.md` (not kebab-case).

## Input and Output Rules

- Final file format is a fenced metadata block using ```runebound with TOML content.
- `type` in metadata is `npc` or `location` depending on entity.
- Location changes must use `npc travel to <location>`.

## Validation

- Reject empty NPC/location names.
- Enforce `sex` as `male|female`.
- Normalize unknown/blank text values to `Unknown` and blank carrying to `["Unknown"]`.

## Future Commands (not in MVP)

- `npc search`
- `npc template`
