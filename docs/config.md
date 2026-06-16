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

### Implementation notes (wizard internals)

The wizard runs as a separate dispatch route, not through the command registry: while `onboarding.active`, input is intercepted by `try_execute_onboarding` (`core/src/command.rs`) before registry dispatch. This carries two invariants that have regressed before:

- **Seed shown fields from effective config, identically across entry points.** Every flow that reaches a menu (`start setup`, `setup vault`, `setup llm`, `setup model`) must seed the fields its prompt displays from `load_effective(...)`. The Ollama menu renders `2: Continue with <ollama.base_url>`; because `OnboardingSession`'s default base URL is `http://127.0.0.1:11434`, seeding only "when empty" never picks up the configured server. Seed unconditionally so the prompt reflects the saved config.
- **`cancel` must exit the wizard.** Setup verbs live in `try_execute_onboarding`, so the desktop `cancel` handler never runs during setup. Both `cancel` and `cancel setup` reset onboarding.

Full dispatch and context model: `docs/command-contexts.md`.

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
- `ollama.num_ctx`
- `ui.confirm_soft_delete`
- `ui.show_inline_help`
- `generation.verbosity`

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
- `ollama.num_ctx`:
  - Context window (tokens) sent to Ollama; defaults to 8192.
  - Must be at least 512. Raise it if you reference many/large documents and have the VRAM; lower it on constrained hardware.
- `generation.verbosity`:
  - How much prose the LLM writes for narrative/descriptive fields (background, history, descriptions, agendas, tensions, abilities, …).
  - One of `"brief"` (1-2 sentences), `"medium"` (3-4 sentences, the default), or `"verbose"` (5-7 sentences).
  - Applied as an authoritative detail directive appended to every generation and field-reroll prompt; it overrides the prompts' baseline per-field sentence counts. Structural fields (e.g. `tone`, `symbol_description`, `exports`) keep their fixed shape regardless.
  - Takes effect on the next generation/reroll (config is re-read per call); no restart needed.

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
num_ctx = 8192

[ui]
confirm_soft_delete = true
show_inline_help = true

[generation]
verbosity = "medium"  # "brief" | "medium" | "verbose"
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
- Check recommended vault directories (`npcs/`, `locations/`, `.trash/npcs/`, `.trash/locations/`).
- Check Ollama response timing and timeout suitability.
- Provide explicit fix steps for each failure.

## Non-Goals (Current MVP)

- No workspace-level config overrides.
- No multi-vault profile management.
- No compatibility migration layer for old workspace config files.
