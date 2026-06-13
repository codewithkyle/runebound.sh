# Config Command Plan

## Goals

- Do not assume vault location.
- Do not assume Ollama host/port.
- Provide a smooth first-time setup flow.
- Store config in TOML at one global location.

## Config File Location

- Global config (machine-level):
  - Linux: `~/.config/runebound.sh/config.toml`
  - Windows: `%APPDATA%\\runebound.sh\\config.toml`
- No workspace config is used.
- Existing `.runebound.sh/config.toml` files are ignored.
- Effective config precedence is:
  - command flags (when applicable)
  - global config
  - built-in defaults

## First-Time Setup

### Trigger

- If required config is missing or invalid, show startup guidance and suggest setup bootstrap commands.

### Wizard steps

1. Ask for vault path.
2. Ask for Ollama base URL (default `http://127.0.0.1:11434`).
3. Test Ollama connection and fetch models.
4. Ask for default model.
5. Save config and display summary.

### Setup command

- `start setup` runs setup manually at any time.

## Command Surface (v1)

- `config`
  - Show concise help for config subcommands.
- `start setup`
  - Run first-time setup wizard and save global config.
- `config show`
  - Show effective global config and config path.
- `config test`
  - Full diagnostics and recommended fixes.

## Supported Keys (v1)

- `vault.path`
- `vault.autoscan_on_start`
- `ollama.base_url`
- `ollama.model`
- `ollama.timeout_seconds`
- `ui.confirm_soft_delete`
- `ui.show_inline_help`

## Validation Rules

- `vault.path`:
  - Path resolves to an absolute path.
  - Path exists and is writable.
  - Must remain inside normal filesystem boundaries.
- `ollama.base_url`:
  - Must be valid URL.
  - Should pass reachability check during setup/test.
- `ollama.model`:
  - Warn if not currently available in Ollama.
  - Keep as non-fatal to allow offline setup.

## Suggested TOML Schema

```toml
version = 1

[vault]
path = "/path/to/Obsidian/Vault"
autoscan_on_start = true

[ollama]
base_url = "http://127.0.0.1:11434"
model = "llama3.1:8b"
timeout_seconds = 120

[ui]
confirm_soft_delete = true
show_inline_help = true
```

## Output and Error Style

- Keep messages short and actionable.
- Include suggested correction when possible.
- Example errors:
  - `Invalid key 'ollama.url'. Did you mean 'ollama.base_url'?`
  - `vault.path is not writable: /path/to/vault`
  - `Cannot reach Ollama at http://127.0.0.1:11434 (timeout)`

## Test Semantics

### `config test` (full)

- Verify required keys are set.
- Verify vault path exists/writable.
- Verify Ollama endpoint reachable.
- Verify configured model exists (warning-only if missing).
- Validate permissions and path normalization.
- Check recommended vault directories (`npcs/`, `.trash/npcs/`).
- Check Ollama response timing and timeout suitability.
- Provide explicit fix steps for each failure.

## Non-Goals (Current MVP)

- No workspace-level config overrides.
- No multi-vault profile management.
- No compatibility migration layer for old workspace config files.
