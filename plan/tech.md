# Technical Plan

## Goals

- Build a Rust-first DnD assistant with a terminal UI.
- Use a local LLM with Ollama as the default provider.
- Store canonical content as markdown files in an Obsidian vault.
- Use SQLite as an index and query layer, not canonical storage.

## Core Decisions

- Language: Rust
- Database: SQLite
- Local model provider (v1): Ollama
- Content format: TOML frontmatter + markdown body
- Interface priority: TUI first, Tauri later
- Command parsing: `clap` for subcommands/flags, invoked from TUI input (not only process args)

## Architecture

### Crates

- `core`: domain models, vault I/O, indexing, LLM client, generation pipeline
- `tui`: command line interaction, autocomplete, list/detail views
- `tauri` (later): desktop shell that reuses `core`

### Core modules

- `config`: app config, vault root, ollama endpoint, defaults
- `vault`: read/write markdown files, parse TOML frontmatter, path resolution
- `index`: SQLite schema, migrations, lookup/search APIs
- `llm`: provider abstraction + Ollama adapter
- `npc`: prompt templates, create/edit logic, reference handling
- `commands`: `clap` command definitions and parser entrypoints reusable by TUI (and later real CLI if needed)

## Data Model Strategy

### Canonical content

- NPC data lives in markdown files under the vault.
- Suggested layout:
  - `vault/npcs/<slug>.md`
  - `vault/.trash/npcs/<slug>.md` for soft delete

### Frontmatter format

- Use TOML frontmatter at the top of each markdown file.
- Required metadata for NPC v1:
  - `id`
  - `type` (`npc`)
  - `name`
  - `created_at`
  - `updated_at`
- Optional metadata for NPC v1:
  - `tags`
  - `source_refs`
  - `system.version`

### SQLite role

- Keep an index for fast lookup/search and CLI listing.
- Store derived metadata and searchable text.
- Rebuild or refresh index from vault files as needed.

## LLM Integration (Ollama)

- Connect through Ollama local HTTP API.
- Configurable values:
  - base URL (default `http://127.0.0.1:11434`)
  - model name
  - temperature/token options
- Generation flow:
  1. Collect user prompt
  2. Resolve referenced vault files
  3. Build structured NPC prompt template
  4. Request completion from Ollama
  5. Save output to markdown + update SQLite index

## Reference Resolution

- Support path references in prompts with `@vault/...` syntax.
- Also support explicit refs via command flags.
- Inject referenced content into generation context with source path labels.

## Soft Delete

- `npc delete` moves file to `vault/.trash/npcs/`.
- Update index to mark item deleted or remove from active listings.
- No hard delete in v1.

## v1 Scope

- NPC workflows only
- Command autocomplete and typeahead for `npc` subcommands
- File save/load + index sync + Ollama generation
