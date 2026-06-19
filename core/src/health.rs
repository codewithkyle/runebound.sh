use std::time::Duration;

use anyhow::{Result, anyhow};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::{AppConfig, required_issues};
use crate::db;
use crate::vault::{Vault, is_path_writable};

/// Live state of the configured Ollama server, used by the boot flow and status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaHealth {
    /// The server answered `/api/tags` successfully.
    pub reachable: bool,
    /// The model configured in the app is present in the server's model list.
    pub model_available: bool,
    /// Human-readable summary suitable for a status/spinner line.
    pub detail: String,
}

/// Raw outcome of probing an Ollama server's `/api/tags` endpoint.
///
/// This is the single shared transport used by the boot, status, ping, and setup
/// flows. Higher-level helpers (`check_ollama_health`, `check_ollama_reachability`,
/// and the setup model probe) format their own presentation from this result.
#[derive(Debug, Clone)]
pub struct OllamaProbe {
    /// The server answered `/api/tags` with `200 OK`.
    pub reachable: bool,
    /// Model names advertised by the server (empty when unreachable/unreadable).
    pub models: Vec<String>,
    /// Failure reason when the probe did not cleanly return a model list.
    /// `None` only when the server returned a readable list.
    pub error: Option<String>,
}

/// Probe an Ollama server for its advertised model list.
///
/// Performs the `/api/tags` GET shared by boot, status, ping, and setup. Never
/// panics: transport, HTTP-status, and body-parse failures are reported via
/// `error`. A `200` with an unreadable body yields `reachable = true` but an
/// `error`, matching the prior boot/ping semantics.
pub async fn probe_ollama(base_url: &str, timeout_seconds: u64) -> OllamaProbe {
    let base = base_url.trim_end_matches('/');
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            return OllamaProbe {
                reachable: false,
                models: Vec::new(),
                error: Some(format!("failed to create HTTP client: {err}")),
            };
        }
    };

    let response = match client.get(format!("{base}/api/tags")).send().await {
        Ok(response) => response,
        Err(_) => {
            return OllamaProbe {
                reachable: false,
                models: Vec::new(),
                error: Some(format!("could not reach the Ollama server at {base_url}")),
            };
        }
    };

    if response.status() != StatusCode::OK {
        return OllamaProbe {
            reachable: false,
            models: Vec::new(),
            error: Some(format!("the Ollama server returned {}", response.status())),
        };
    }

    let value = match response.json::<serde_json::Value>().await {
        Ok(value) => value,
        Err(_) => {
            return OllamaProbe {
                reachable: true,
                models: Vec::new(),
                error: Some("the Ollama server response could not be read".to_string()),
            };
        }
    };

    let models = value
        .get("models")
        .and_then(|models| models.as_array())
        .map(|models| {
            models
                .iter()
                .filter_map(|model| {
                    model
                        .get("name")
                        .and_then(|name| name.as_str())
                        .map(String::from)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    OllamaProbe {
        reachable: true,
        models,
        error: None,
    }
}

/// Probe the configured Ollama server and verify the configured model exists.
///
/// Uses a short timeout so a dead server does not stall the boot sequence.
pub async fn check_ollama_health(config: &AppConfig, timeout_seconds: u64) -> OllamaHealth {
    let base = config.ollama.base_url.trim();
    if base.is_empty() {
        return OllamaHealth {
            reachable: false,
            model_available: false,
            detail: "no Ollama server is configured".to_string(),
        };
    }
    if validate_ollama_url(base).is_err() {
        return OllamaHealth {
            reachable: false,
            model_available: false,
            detail: format!("invalid Ollama URL: {base}"),
        };
    }

    let probe = probe_ollama(base, timeout_seconds).await;
    if !probe.reachable {
        return OllamaHealth {
            reachable: false,
            model_available: false,
            detail: probe
                .error
                .unwrap_or_else(|| format!("could not reach the Ollama server at {base}")),
        };
    }
    if let Some(error) = probe.error {
        // Server answered but the model list could not be read.
        return OllamaHealth {
            reachable: true,
            model_available: false,
            detail: error,
        };
    }

    match config.ollama.model.as_deref() {
        Some(model) if !model.trim().is_empty() => {
            if probe.models.iter().any(|candidate| candidate == model) {
                OllamaHealth {
                    reachable: true,
                    model_available: true,
                    detail: format!("model {model} is available"),
                }
            } else {
                OllamaHealth {
                    reachable: true,
                    model_available: false,
                    detail: format!("the configured model {model} is no longer available"),
                }
            }
        }
        _ => OllamaHealth {
            reachable: true,
            model_available: false,
            detail: "no model is configured".to_string(),
        },
    }
}

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

pub async fn run_doctor_checks(config: &AppConfig) -> CheckReport {
    let mut report = run_quick_checks(config).await;
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
    // The explicit diagnostic honors the configured timeout (the user asked to
    // test the connection thoroughly), unlike the fast boot/setup probes.
    let probe = probe_ollama(&config.ollama.base_url, config.ollama.timeout_seconds).await;
    if probe.reachable {
        CheckItem {
            name: "ollama connection".to_string(),
            ok: true,
            detail: "reachable".to_string(),
        }
    } else {
        CheckItem {
            name: "ollama connection".to_string(),
            ok: false,
            detail: probe.error.unwrap_or_else(|| "unreachable".to_string()),
        }
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
            detail: format!(
                "{} (+ matching .trash) ensured",
                crate::vault::ENTITY_DIRS.join(", ")
            ),
        },
        Err(err) => CheckItem {
            name: "vault directories".to_string(),
            ok: false,
            detail: err.to_string(),
        },
    }
}
