use std::path::Path;
use std::time::Duration;

use anyhow::{Result, anyhow};
use reqwest::StatusCode;
use url::Url;

use crate::config::{AppConfig, required_issues};
use crate::db;
use crate::vault::{Vault, is_path_writable};

#[derive(Debug, Clone)]
pub struct CheckItem {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct CheckReport {
    pub items: Vec<CheckItem>,
}

impl CheckReport {
    pub fn is_ok(&self) -> bool {
        self.items.iter().all(|item| item.ok)
    }
}

pub fn validate_ollama_url(base_url: &str) -> Result<()> {
    let parsed = Url::parse(base_url).map_err(|e| anyhow!("invalid ollama.base_url: {e}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(anyhow!("ollama.base_url must use http or https"));
    }
    if parsed.host_str().is_none() {
        return Err(anyhow!("ollama.base_url is missing host"));
    }
    Ok(())
}

pub async fn run_quick_checks(config: &AppConfig) -> CheckReport {
    let mut items = Vec::new();

    let required = required_issues(config);
    if required.is_empty() {
        items.push(CheckItem {
            name: "required config".to_string(),
            ok: true,
            detail: "all required keys are present".to_string(),
        });
    } else {
        items.push(CheckItem {
            name: "required config".to_string(),
            ok: false,
            detail: required.join("; "),
        });
    }

    items.push(check_vault(config));
    items.push(check_ollama_config(config));
    items.push(check_ollama_reachability(config).await);
    items.push(check_database().await);

    CheckReport { items }
}

pub async fn run_doctor_checks(config: &AppConfig, workspace_root: &Path) -> CheckReport {
    let mut report = run_quick_checks(config).await;

    report.items.push(CheckItem {
        name: "workspace root".to_string(),
        ok: true,
        detail: workspace_root.display().to_string(),
    });

    report.items.push(check_vault_structure(config));

    report
}

fn check_vault(config: &AppConfig) -> CheckItem {
    let Some(path) = config.vault.path.as_ref() else {
        return CheckItem {
            name: "vault.path".to_string(),
            ok: false,
            detail: "not configured".to_string(),
        };
    };

    if !path.exists() {
        return CheckItem {
            name: "vault.path".to_string(),
            ok: false,
            detail: format!("path does not exist: {}", path.display()),
        };
    }

    if !path.is_dir() {
        return CheckItem {
            name: "vault.path".to_string(),
            ok: false,
            detail: format!("path is not a directory: {}", path.display()),
        };
    }

    match is_path_writable(path) {
        Ok(()) => CheckItem {
            name: "vault.path".to_string(),
            ok: true,
            detail: format!("read/write ok at {}", path.display()),
        },
        Err(err) => CheckItem {
            name: "vault.path".to_string(),
            ok: false,
            detail: err.to_string(),
        },
    }
}

fn check_ollama_config(config: &AppConfig) -> CheckItem {
    match validate_ollama_url(&config.ollama.base_url) {
        Ok(()) => CheckItem {
            name: "ollama.base_url".to_string(),
            ok: true,
            detail: config.ollama.base_url.clone(),
        },
        Err(err) => CheckItem {
            name: "ollama.base_url".to_string(),
            ok: false,
            detail: err.to_string(),
        },
    }
}

async fn check_ollama_reachability(config: &AppConfig) -> CheckItem {
    let url = format!("{}/api/tags", config.ollama.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.ollama.timeout_seconds))
        .build();

    let Ok(client) = client else {
        return CheckItem {
            name: "ollama connection".to_string(),
            ok: false,
            detail: "failed to create HTTP client".to_string(),
        };
    };

    match client.get(url).send().await {
        Ok(response) => {
            if response.status() == StatusCode::OK {
                CheckItem {
                    name: "ollama connection".to_string(),
                    ok: true,
                    detail: "reachable".to_string(),
                }
            } else {
                CheckItem {
                    name: "ollama connection".to_string(),
                    ok: false,
                    detail: format!("unexpected HTTP status: {}", response.status()),
                }
            }
        }
        Err(err) => CheckItem {
            name: "ollama connection".to_string(),
            ok: false,
            detail: err.to_string(),
        },
    }
}

async fn check_database() -> CheckItem {
    match db::init_database().await {
        Ok(db) => match db::health_check(&db.pool).await {
            Ok(()) => CheckItem {
                name: "sqlite".to_string(),
                ok: true,
                detail: format!("ready at {}", db.path.display()),
            },
            Err(err) => CheckItem {
                name: "sqlite".to_string(),
                ok: false,
                detail: err.to_string(),
            },
        },
        Err(err) => CheckItem {
            name: "sqlite".to_string(),
            ok: false,
            detail: err.to_string(),
        },
    }
}

fn check_vault_structure(config: &AppConfig) -> CheckItem {
    let Some(path) = config.vault.path.clone() else {
        return CheckItem {
            name: "vault directories".to_string(),
            ok: false,
            detail: "vault.path is not configured".to_string(),
        };
    };

    let vault = Vault::new(path);
    match vault.ensure_structure() {
        Ok(()) => CheckItem {
            name: "vault directories".to_string(),
            ok: true,
            detail: "npcs, locations, items, factions ensured".to_string(),
        },
        Err(err) => CheckItem {
            name: "vault directories".to_string(),
            ok: false,
            detail: err.to_string(),
        },
    }
}
