use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

const APP_DIR_NAME: &str = "dnd-assistant";

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub confirm_soft_delete: bool,
    #[serde(default = "default_true")]
    pub show_inline_help: bool,
}

#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub global: PathBuf,
    pub workspace: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub effective: AppConfig,
    pub global_exists: bool,
    pub workspace_exists: bool,
    pub paths: ConfigPaths,
}

#[derive(Debug, Clone, Copy)]
pub enum ConfigScope {
    Global,
    Workspace,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialAppConfig {
    version: Option<u32>,
    vault: Option<PartialVaultConfig>,
    ollama: Option<PartialOllamaConfig>,
    ui: Option<PartialUiConfig>,
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
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialUiConfig {
    confirm_soft_delete: Option<bool>,
    show_inline_help: Option<bool>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            vault: VaultConfig::default(),
            ollama: OllamaConfig::default(),
            ui: UiConfig::default(),
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
    let workspace_exists = paths.workspace.exists();

    if global_exists {
        let partial = load_partial_file(&paths.global)?;
        apply_partial(&mut config, partial);
    }
    if workspace_exists {
        let partial = load_partial_file(&paths.workspace)?;
        apply_partial(&mut config, partial);
    }

    Ok(LoadedConfig {
        effective: config,
        global_exists,
        workspace_exists,
        paths,
    })
}

pub fn save_config(workspace_root: &Path, scope: ConfigScope, config: &AppConfig) -> Result<PathBuf> {
    let paths = config_paths(workspace_root)?;
    let target = match scope {
        ConfigScope::Global => paths.global,
        ConfigScope::Workspace => paths.workspace,
    };

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let serialized = toml::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(&target, serialized)
        .with_context(|| format!("failed to write config file {}", target.display()))?;

    Ok(target)
}

pub fn determine_default_write_scope(loaded: &LoadedConfig) -> ConfigScope {
    if loaded.workspace_exists {
        ConfigScope::Workspace
    } else {
        ConfigScope::Global
    }
}

pub fn config_paths(workspace_root: &Path) -> Result<ConfigPaths> {
    let config_base = dirs::config_dir().ok_or_else(|| anyhow!("unable to find config directory"))?;

    let global = config_base.join(APP_DIR_NAME).join("config.toml");
    let workspace = workspace_root.join(".dnd-assistant").join("config.toml");

    Ok(ConfigPaths { global, workspace })
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
    }

    if let Some(ui) = partial.ui {
        if let Some(confirm_soft_delete) = ui.confirm_soft_delete {
            base.ui.confirm_soft_delete = confirm_soft_delete;
        }
        if let Some(show_inline_help) = ui.show_inline_help {
            base.ui.show_inline_help = show_inline_help;
        }
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
