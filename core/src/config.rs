use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

const APP_DIR_NAME: &str = "runebound.sh";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub vault: VaultConfig,
    #[serde(default)]
    pub ollama: OllamaConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub generation: GenerationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default = "default_true")]
    pub autoscan_on_start: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_ollama_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default = "default_ollama_num_ctx")]
    pub num_ctx: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub confirm_soft_delete: bool,
    #[serde(default = "default_true")]
    pub show_inline_help: bool,
}

/// How much detail the LLM should write for generated narrative/descriptive
/// fields. Tunes the length of backgrounds, histories, descriptions, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verbosity {
    /// 1-2 tight sentences per narrative field.
    Brief,
    /// 3-4 substantive sentences — the default middle ground.
    #[default]
    Medium,
    /// 5-7 vivid, detail-rich sentences.
    Verbose,
}

impl Verbosity {
    pub fn as_str(self) -> &'static str {
        match self {
            Verbosity::Brief => "brief",
            Verbosity::Medium => "medium",
            Verbosity::Verbose => "verbose",
        }
    }

    /// Parse a user-typed value (case-insensitive). Returns `None` for anything
    /// other than the three known levels so callers can surface a clear error.
    pub fn parse(value: &str) -> Option<Verbosity> {
        match value.trim().to_ascii_lowercase().as_str() {
            "brief" => Some(Verbosity::Brief),
            "medium" => Some(Verbosity::Medium),
            "verbose" => Some(Verbosity::Verbose),
            _ => None,
        }
    }

    /// The accepted values, for usage/typeahead text.
    pub const ALL: [Verbosity; 3] = [Verbosity::Brief, Verbosity::Medium, Verbosity::Verbose];
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerationConfig {
    /// Detail level for generated prose. Defaults to [`Verbosity::Medium`].
    #[serde(default)]
    pub verbosity: Verbosity,
}

#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub global: PathBuf,
    pub calendar: PathBuf,
    pub entities: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub effective: AppConfig,
    pub global_exists: bool,
    pub paths: ConfigPaths,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialAppConfig {
    version: Option<u32>,
    vault: Option<PartialVaultConfig>,
    ollama: Option<PartialOllamaConfig>,
    ui: Option<PartialUiConfig>,
    generation: Option<PartialGenerationConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialVaultConfig {
    path: Option<PathBuf>,
    autoscan_on_start: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialOllamaConfig {
    base_url: Option<String>,
    model: Option<String>,
    timeout_seconds: Option<u64>,
    num_ctx: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialUiConfig {
    confirm_soft_delete: Option<bool>,
    show_inline_help: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialGenerationConfig {
    verbosity: Option<Verbosity>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            vault: VaultConfig::default(),
            ollama: OllamaConfig::default(),
            ui: UiConfig::default(),
            generation: GenerationConfig::default(),
        }
    }
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            path: None,
            autoscan_on_start: true,
        }
    }
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: default_ollama_base_url(),
            model: None,
            timeout_seconds: default_ollama_timeout_seconds(),
            num_ctx: default_ollama_num_ctx(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            confirm_soft_delete: true,
            show_inline_help: true,
        }
    }
}

pub fn load_effective(workspace_root: &Path) -> Result<LoadedConfig> {
    let paths = config_paths(workspace_root)?;

    let mut config = AppConfig::default();
    let global_exists = paths.global.exists();

    if global_exists {
        let partial = load_partial_file(&paths.global)?;
        apply_partial(&mut config, partial);
    }

    Ok(LoadedConfig {
        effective: config,
        global_exists,
        paths,
    })
}

pub fn save_config(workspace_root: &Path, config: &AppConfig) -> Result<PathBuf> {
    let paths = config_paths(workspace_root)?;
    let target = paths.global;

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let serialized = toml::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(&target, serialized)
        .with_context(|| format!("failed to write config file {}", target.display()))?;

    Ok(target)
}

/// Persist newly-added config sections that an older config file predates.
///
/// Currently this backfills `[generation]`: if the config file exists but has
/// no `[generation]` section, the effective config (with defaults filled in) is
/// written back so the section becomes visible and editable on disk. No-op when
/// the file is absent (first-time setup writes a complete file) or the section
/// is already present. Returns `Ok(true)` when it wrote.
pub fn ensure_config_sections_persisted(workspace_root: &Path) -> Result<bool> {
    let paths = config_paths(workspace_root)?;
    if !paths.global.exists() {
        return Ok(false);
    }

    let partial = load_partial_file(&paths.global)?;
    if partial.generation.is_some() {
        return Ok(false);
    }

    let loaded = load_effective(workspace_root)?;
    save_config(workspace_root, &loaded.effective)?;
    Ok(true)
}

pub fn config_paths(_workspace_root: &Path) -> Result<ConfigPaths> {
    let config_base =
        dirs::config_dir().ok_or_else(|| anyhow!("unable to find config directory"))?;

    let app_root = config_base.join(APP_DIR_NAME);
    let global = app_root.join("config.toml");
    let calendar = app_root.join("calendar.toml");
    let entities = app_root.join("entities");

    Ok(ConfigPaths {
        global,
        calendar,
        entities,
    })
}

pub fn required_issues(config: &AppConfig) -> Vec<String> {
    let mut issues = Vec::new();

    match &config.vault.path {
        Some(path) if path.as_os_str().is_empty() => issues.push("vault.path is empty".to_string()),
        None => issues.push("vault.path is not configured".to_string()),
        _ => {}
    }

    if config.ollama.base_url.trim().is_empty() {
        issues.push("ollama.base_url is empty".to_string());
    }

    if config.ollama.timeout_seconds == 0 {
        issues.push("ollama.timeout_seconds must be greater than 0".to_string());
    }

    if config.ollama.num_ctx < 512 {
        issues.push("ollama.num_ctx must be at least 512".to_string());
    }

    issues
}

pub fn validate_for_runtime(config: &AppConfig) -> Result<()> {
    let issues = required_issues(config);
    if !issues.is_empty() {
        bail!("config is incomplete:\n- {}", issues.join("\n- "));
    }
    Ok(())
}

fn load_partial_file(path: &Path) -> Result<PartialAppConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;

    let parsed: PartialAppConfig =
        toml::from_str(&raw).with_context(|| format!("invalid TOML in {}", path.display()))?;
    Ok(parsed)
}

fn apply_partial(base: &mut AppConfig, partial: PartialAppConfig) {
    if let Some(version) = partial.version {
        base.version = version;
    }

    if let Some(vault) = partial.vault {
        if let Some(path) = vault.path {
            base.vault.path = Some(path);
        }
        if let Some(autoscan_on_start) = vault.autoscan_on_start {
            base.vault.autoscan_on_start = autoscan_on_start;
        }
    }

    if let Some(ollama) = partial.ollama {
        if let Some(base_url) = ollama.base_url {
            base.ollama.base_url = base_url;
        }
        if let Some(model) = ollama.model {
            base.ollama.model = Some(model);
        }
        if let Some(timeout_seconds) = ollama.timeout_seconds {
            base.ollama.timeout_seconds = timeout_seconds;
        }
        if let Some(num_ctx) = ollama.num_ctx {
            base.ollama.num_ctx = num_ctx;
        }
    }

    if let Some(ui) = partial.ui {
        if let Some(confirm_soft_delete) = ui.confirm_soft_delete {
            base.ui.confirm_soft_delete = confirm_soft_delete;
        }
        if let Some(show_inline_help) = ui.show_inline_help {
            base.ui.show_inline_help = show_inline_help;
        }
    }

    if let Some(generation) = partial.generation
        && let Some(verbosity) = generation.verbosity
    {
        base.generation.verbosity = verbosity;
    }
}

fn default_version() -> u32 {
    1
}

fn default_true() -> bool {
    true
}

fn default_ollama_base_url() -> String {
    "http://127.0.0.1:11434".to_string()
}

fn default_ollama_timeout_seconds() -> u64 {
    120
}

fn default_ollama_num_ctx() -> u32 {
    8192
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_verbosity_is_medium() {
        assert_eq!(AppConfig::default().generation.verbosity, Verbosity::Medium);
    }

    #[test]
    fn missing_generation_section_defaults_to_medium() {
        // Backward compatibility: existing config files without [generation].
        let config: AppConfig = toml::from_str("").expect("parse empty config");
        assert_eq!(config.generation.verbosity, Verbosity::Medium);
    }

    #[test]
    fn verbosity_parses_from_lowercase_toml() {
        let config: AppConfig =
            toml::from_str("[generation]\nverbosity = \"verbose\"\n").expect("parse config");
        assert_eq!(config.generation.verbosity, Verbosity::Verbose);
    }

    #[test]
    fn verbosity_serializes_to_lowercase() {
        let mut config = AppConfig::default();
        config.generation.verbosity = Verbosity::Brief;
        let serialized = toml::to_string(&config).expect("serialize config");
        assert!(
            serialized.contains("verbosity = \"brief\""),
            "expected lowercase verbosity, got:\n{serialized}"
        );
    }

    #[test]
    fn apply_partial_overrides_verbosity() {
        let mut base = AppConfig::default();
        let partial = PartialAppConfig {
            generation: Some(PartialGenerationConfig {
                verbosity: Some(Verbosity::Verbose),
            }),
            ..Default::default()
        };
        apply_partial(&mut base, partial);
        assert_eq!(base.generation.verbosity, Verbosity::Verbose);
    }

    #[test]
    fn apply_partial_without_generation_keeps_default() {
        let mut base = AppConfig::default();
        apply_partial(&mut base, PartialAppConfig::default());
        assert_eq!(base.generation.verbosity, Verbosity::Medium);
    }

    #[test]
    fn verbosity_parses_known_levels_case_insensitively() {
        assert_eq!(Verbosity::parse("brief"), Some(Verbosity::Brief));
        assert_eq!(Verbosity::parse("MEDIUM"), Some(Verbosity::Medium));
        assert_eq!(Verbosity::parse("  Verbose "), Some(Verbosity::Verbose));
    }

    #[test]
    fn verbosity_parse_rejects_unknown_values() {
        assert_eq!(Verbosity::parse("lots"), None);
        assert_eq!(Verbosity::parse(""), None);
    }
}
