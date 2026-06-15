% Publish Feature Plan

# Objective

Replace the embedded `runebound` blocks in Obsidian markdown files with a canonical TOML store inside the app config directory, and add a one-way `publish` workflow that renders user-friendly markdown on demand.

# Requirements

1. **Canonical Storage**
   - Persist each entity (NPC, location, faction, item, etc.) as TOML under `~/.config/runebound.sh/entities/<kind>/<slug>.toml`.
   - CRUD operations (create/edit/save) must read/write these TOML files, not the vault markdown. Runebound blocks are no longer needed in Obsidian files.
   - It is acceptable to nuke existing test data; no migration tooling is required for now.

2. **Publish Command**
   - Add `publish <name>` command (desktop) that resolves the entity by name/slug via existing lookup/suggestions. Users should not have to specify entity type.
   - Flow:
     1. Load entity data from TOML.
     2. Render a human-readable markdown layout tailored to the entity type.
     3. Determine target vault path (e.g., `npcs/<slug>.md`, configurable via stored metadata or convention).
     4. If the file already exists, prompt `Overwrite <path>? [y/N]`. Default to “No” unless the user confirms.
     5. On confirmation (or if file absent), write the rendered markdown, overwriting existing content. Publishing is one-way: no attempt to preserve prior user edits.
   - Future flags (not required now): `--force` to skip prompts, `publish all` batch mode.

3. **Markdown Renderers**
   - Implement per-entity renderers that convert structured data into readable Obsidian sections. Example for NPC:
     - `# Name`
     - Metadata table (race, occupation, age, sex, location)
     - Sections for Appearance, Background, Goals, Secrets
     - Bullet list for Carrying
   - Skip sections when values are `Unknown`. Ensure markdown is clean, consistent, and easy to scan.
   - Renderers should live in a shared module (e.g., `desktop/src-tauri/src/publish/renderers.rs` or `runebound-models::render`).

4. **CLI/UX Updates**
   - Add `publish` command help in `docs/cli.md`, describing confirmation behavior and one-way nature.
   - Autocomplete should offer entity names; follow existing suggestion patterns.
   - Mention that Obsidian edits are not synced back; users must re-run `publish` after in-app edits.

5. **Code Cleanup**
   - Remove/disable legacy paths that parse runebound blocks from Obsidian files (vault sync, imports) since structured data now lives in config TOML.
   - Ensure any references to `runebound` blocks either point to the canonical files or are deprecated.
   - Vault sync must read from the canonical `EntityStore` TOML files exclusively; manual vault edits are never imported.

6. **Testing**
   - Unit tests for renderer output (snapshot or string assertions) per entity type.
   - Command-level tests for publish logic: prompt handling, file overwrite, path resolution.
   - Ensure existing entity persistence tests are updated to expect TOML storage.

7. **Documentation**
   - Update `docs/feature-development.md` and `docs/cli.md` to explain the new storage model and publish workflow.
   - If helpful, add a `docs/publish.md` detailing templates, limitations, and future ideas (optional but recommended).
