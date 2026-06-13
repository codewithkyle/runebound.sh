# Config Command Plan

## Goals

- Do not assume vault location.
- Do not assume Ollama host/port.
- Provide a smooth first-time setup flow.
- Store config in TOML with clear override precedence.

## Config Files and Precedence

- Global config (machine-level):
  - Linux: `~/.config/dnd-assistant/config.toml`
  - Windows: `%APPDATA%\\dnd-assistant\\config.toml`
- Workspace config (project-level override):
  - `.dnd-assistant/config.toml`
- Precedence (highest to lowest):
  - Command flags
  - Workspace config
  - Global config
  - Built-in defaults

## First-Time Setup

### Trigger

- If required config is missing or invalid, prompt user into setup wizard.

### Wizard steps

1. Ask for vault path.
2. Ask for Ollama base URL (default `http://127.0.0.1:11434`).
3. Test Ollama connection and fetch models.
4. Ask for default model.
5. Save config and display summary.

### Setup command

- `config init` runs setup manually at any time.

## Command Surface (v1)

- `config`
  - Show concise help and effective summary.
- `config init`
  - Run first-time setup wizard.
- `config show`
  - Show effective merged config.
- `config where`
  - Show config file paths and existence status.
- `config get <key>`
  - Print one resolved value.
- `config set <key> <value>`
  - Write one value to selected scope.
- `config unset <key>`
  - Remove one key from selected scope.
- `config test`
  - Quick validation for required settings and connectivity.
- `config doctor`
  - Deep diagnostics and recommended fixes.
- `config reset`
  - Interactive reset/reinitialize for selected scope.

## Scope Flags

- Supported on write commands:
  - `--global`
  - `--workspace`
- Default write behavior:
  - If workspace config exists, write there.
  - Otherwise write global config.

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

## Test and Doctor Semantics

### `config test` (quick)

- Verify required keys are set.
- Verify vault path exists/writable.
- Verify Ollama endpoint reachable.
- Verify configured model exists (warning-only if missing).

### `config doctor` (deep)

- Explain effective merge source per key.
- Validate permissions and path normalization.
- Check recommended vault directories (`npcs/`, `.trash/npcs/`).
- Check Ollama response timing and timeout suitability.
- Provide explicit fix steps for each failure.
