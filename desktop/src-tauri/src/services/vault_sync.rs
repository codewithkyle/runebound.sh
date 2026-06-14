use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use dnd_core::config::load_effective;
use dnd_core::npc::{UNKNOWN_LOCATION, normalize_markdown_file_stem, now_timestamp, slugify};
use dnd_core::vault::Vault;

use crate::app_state::AppState;
use crate::repositories::{db, DocumentRepository, FactionRepository, LocationRepository, NpcRepository};
use crate::utils::{
    normalize_exports, normalize_faction_kind_type, normalize_location_danger_level,
    normalize_location_kind_type, normalize_relative_path_for_storage, normalize_unknown_list,
    normalize_unknown_text,
};

pub struct VaultSyncService;

impl VaultSyncService {
    pub async fn sync_from_vault(&self, state: &AppState) -> Result<(), String> {
        let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
        if !loaded.effective.vault.autoscan_on_start {
            return Ok(());
        }

        let Some(vault_path) = loaded.effective.vault.path.clone() else {
            return Ok(());
        };

        let vault = Vault::new(vault_path);
        vault.ensure_structure().map_err(|err| err.to_string())?;

        let database = state.database();
        let npc_repo = state.npc_repo();
        let location_repo = state.location_repo();
        let faction_repo = state.faction_repo();
        let document_repo = state.document_repo();

        sync_npcs(&vault, database.as_ref(), npc_repo.as_ref(), document_repo.as_ref()).await?;
        sync_locations(&vault, database.as_ref(), location_repo.as_ref(), document_repo.as_ref()).await?;
        sync_factions(&vault, database.as_ref(), faction_repo.as_ref(), document_repo.as_ref()).await?;

        Ok(())
    }
}

async fn sync_npcs(
    vault: &Vault,
    database: &db::Database,
    npc_repo: &dyn NpcRepository,
    document_repo: &dyn DocumentRepository,
) -> Result<(), String> {
    let npc_files = collect_markdown_files_under(vault.root(), "npcs")?;
    let mut scanned_paths = HashSet::new();

    for (relative_path, contents) in npc_files {
        let row = scan_npc_row_from_markdown(&relative_path, &contents);
        scanned_paths.insert(row.vault_path.clone());
        npc_repo.upsert(database, &row).await?;
        document_repo
            .upsert_index(
                database,
                "npc",
                &row.slug,
                Some(&row.name),
                &row.vault_path,
                &row.created_at,
                &row.updated_at,
            )
            .await?;
    }

    let existing = npc_repo.list_all(database).await?;
    for row in existing {
        if row.vault_path.starts_with("npcs/") && !scanned_paths.contains(&row.vault_path) {
            npc_repo.delete_by_id(database, &row.id).await?;
            document_repo
                .delete_by_vault_path(database, &row.vault_path)
                .await?;
        }
    }

    Ok(())
}

async fn sync_locations(
    vault: &Vault,
    database: &db::Database,
    location_repo: &dyn LocationRepository,
    document_repo: &dyn DocumentRepository,
) -> Result<(), String> {
    let location_files = collect_markdown_files_under(vault.root(), "locations")?;
    let mut scanned_paths = HashSet::new();

    for (relative_path, contents) in location_files {
        let row = scan_location_row_from_markdown(&relative_path, &contents);
        scanned_paths.insert(row.vault_path.clone());
        location_repo.upsert(database, &row).await?;
        document_repo
            .upsert_index(
                database,
                "location",
                &row.slug,
                Some(&row.name),
                &row.vault_path,
                &row.created_at,
                &row.updated_at,
            )
            .await?;
    }

    let existing = location_repo.list_all(database).await?;
    for row in existing {
        if row.vault_path.starts_with("locations/") && !scanned_paths.contains(&row.vault_path) {
            location_repo.delete_by_id(database, &row.id).await?;
            document_repo
                .delete_by_vault_path(database, &row.vault_path)
                .await?;
        }
    }

    Ok(())
}

async fn sync_factions(
    vault: &Vault,
    database: &db::Database,
    faction_repo: &dyn FactionRepository,
    document_repo: &dyn DocumentRepository,
) -> Result<(), String> {
    let faction_files = collect_markdown_files_under(vault.root(), "factions")?;
    let mut scanned_paths = HashSet::new();

    for (relative_path, contents) in faction_files {
        let row = scan_faction_row_from_markdown(&relative_path, &contents);
        scanned_paths.insert(row.vault_path.clone());
        faction_repo.upsert(database, &row).await?;
        document_repo
            .upsert_index(
                database,
                "faction",
                &row.slug,
                Some(&row.name),
                &row.vault_path,
                &row.created_at,
                &row.updated_at,
            )
            .await?;
    }

    let existing = faction_repo.list_all(database).await?;
    for row in existing {
        if row.vault_path.starts_with("factions/") && !scanned_paths.contains(&row.vault_path) {
            faction_repo.delete_by_id(database, &row.id).await?;
            document_repo
                .delete_by_vault_path(database, &row.vault_path)
                .await?;
        }
    }

    Ok(())
}

fn collect_markdown_files_under(
    root: &Path,
    relative_dir: &str,
) -> Result<Vec<(String, String)>, String> {
    let base_dir = root.join(relative_dir);
    if !base_dir.exists() {
        return Ok(Vec::new());
    }

    let mut stack = vec![base_dir];
    let mut files = Vec::new();

    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(&current)
            .map_err(|err| format!("failed to read directory {}: {}", current.display(), err))?;

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    eprintln!("startup sync warning: failed to read directory entry: {err}");
                    continue;
                }
            };
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let is_md = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
            if !is_md {
                continue;
            }

            let relative = match path.strip_prefix(root) {
                Ok(relative) => normalize_relative_path_for_storage(&relative.to_string_lossy()),
                Err(_) => continue,
            };
            match fs::read_to_string(&path) {
                Ok(contents) => files.push((relative, contents)),
                Err(err) => {
                    eprintln!(
                        "startup sync warning: failed to read markdown file {}: {}",
                        path.display(),
                        err
                    );
                }
            }
        }
    }

    Ok(files)
}

fn scan_npc_row_from_markdown(relative_path: &str, contents: &str) -> db::NpcRow {
    let parsed = extract_runebound_toml(contents)
        .and_then(|toml_text| toml::from_str::<toml::Value>(&toml_text).ok());
    let now = now_timestamp();
    let fallback_name = file_stem_name(relative_path);

    let name = parsed
        .as_ref()
        .and_then(|value| value.get("name").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&fallback_name)
        .to_string();
    let slug = parsed
        .as_ref()
        .and_then(|value| value.get("slug").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| slugify(&name));
    let id = parsed
        .as_ref()
        .and_then(|value| value.get("id").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| stable_id_from_relative("npc", relative_path));

    let sex = parsed
        .as_ref()
        .and_then(|value| value.get("sex").and_then(toml::Value::as_str))
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|value| value == "male" || value == "female")
        .unwrap_or_else(|| "male".to_string());

    let carrying = parsed
        .as_ref()
        .and_then(|value| value.get("carrying").and_then(toml::Value::as_array))
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| vec!["Unknown".to_string()]);

    db::NpcRow {
        id,
        slug,
        name,
        race: parsed
            .as_ref()
            .and_then(|value| value.get("race").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        occupation: parsed
            .as_ref()
            .and_then(|value| value.get("occupation").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        sex,
        age: parsed
            .as_ref()
            .and_then(|value| value.get("age").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        height: parsed
            .as_ref()
            .and_then(|value| value.get("height").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        weight_lbs: parsed
            .as_ref()
            .and_then(|value| value.get("weight_lbs").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        background: parsed
            .as_ref()
            .and_then(|value| value.get("background").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        want_need: parsed
            .as_ref()
            .and_then(|value| value.get("want_need").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        secret_obstacle: parsed
            .as_ref()
            .and_then(|value| value.get("secret_obstacle").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        carrying: serde_json::to_string(&carrying).unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        location: parsed
            .as_ref()
            .and_then(|value| value.get("location").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| UNKNOWN_LOCATION.to_string()),
        vault_path: normalize_relative_path_for_storage(relative_path),
        created_at: parsed
            .as_ref()
            .and_then(|value| value.get("created_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| now.clone()),
        updated_at: parsed
            .as_ref()
            .and_then(|value| value.get("updated_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or(now),
    }
}

fn scan_location_row_from_markdown(relative_path: &str, contents: &str) -> db::LocationRow {
    let parsed = extract_runebound_toml(contents)
        .and_then(|toml_text| toml::from_str::<toml::Value>(&toml_text).ok());
    let now = now_timestamp();
    let fallback_name = file_stem_name(relative_path);

    let name = parsed
        .as_ref()
        .and_then(|value| value.get("name").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&fallback_name)
        .to_string();
    let slug = parsed
        .as_ref()
        .and_then(|value| value.get("slug").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| slugify(&name));
    let id = parsed
        .as_ref()
        .and_then(|value| value.get("id").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| stable_id_from_relative("loc", relative_path));

    let kind_raw = parsed
        .as_ref()
        .and_then(|value| {
            value
                .get("kind_type")
                .or_else(|| value.get("kind"))
                .and_then(toml::Value::as_str)
        })
        .unwrap_or("other");
    let kind_type = normalize_location_kind_type(kind_raw).unwrap_or_else(|_| "other".to_string());

    let mut kind_custom = parsed
        .as_ref()
        .and_then(|value| value.get("kind_custom").and_then(toml::Value::as_str))
        .map(normalize_unknown_text);
    if kind_type == "other" && kind_custom.is_none() {
        kind_custom = Some("Unknown".to_string());
    }
    if kind_type != "other" {
        kind_custom = None;
    }

    let exports = parsed
        .as_ref()
        .and_then(|value| value.get("exports"))
        .and_then(|raw| {
            if let Some(items) = raw.as_array() {
                let out: Vec<String> = items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(ToString::to_string)
                    .collect();
                return Some(out);
            }
            raw.as_str().map(parse_list_csv)
        })
        .unwrap_or_else(|| vec!["Unknown".to_string()]);
    let exports = normalize_exports(exports);

    db::LocationRow {
        id,
        slug,
        name,
        vault_path: normalize_relative_path_for_storage(relative_path),
        kind_type,
        kind_custom,
        visual_description: parsed
            .as_ref()
            .and_then(|value| value.get("visual_description").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        history_background: parsed
            .as_ref()
            .and_then(|value| value.get("history_background").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        exports: serde_json::to_string(&exports).unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        tone: parsed
            .as_ref()
            .and_then(|value| value.get("tone").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        authority: parsed
            .as_ref()
            .and_then(|value| value.get("authority").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        danger_level: parsed
            .as_ref()
            .and_then(|value| value.get("danger_level").and_then(toml::Value::as_str))
            .map(|value| normalize_location_danger_level(value).unwrap_or_else(|_| "Unknown".to_string()))
            .unwrap_or_else(|| "Unknown".to_string()),
        current_tension: parsed
            .as_ref()
            .and_then(|value| value.get("current_tension").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        created_at: parsed
            .as_ref()
            .and_then(|value| value.get("created_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| now.clone()),
        updated_at: parsed
            .as_ref()
            .and_then(|value| value.get("updated_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or(now),
    }
}

fn scan_faction_row_from_markdown(relative_path: &str, contents: &str) -> db::FactionRow {
    let parsed = extract_runebound_toml(contents)
        .and_then(|toml_text| toml::from_str::<toml::Value>(&toml_text).ok());
    let now = now_timestamp();
    let fallback_name = file_stem_name(relative_path);

    let name = parsed
        .as_ref()
        .and_then(|value| value.get("name").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&fallback_name)
        .to_string();
    let slug = parsed
        .as_ref()
        .and_then(|value| value.get("slug").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| slugify(&name));
    let id = parsed
        .as_ref()
        .and_then(|value| value.get("id").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| stable_id_from_relative("fac", relative_path));

    let kind_raw = parsed
        .as_ref()
        .and_then(|value| {
            value
                .get("kind_type")
                .or_else(|| value.get("kind"))
                .and_then(toml::Value::as_str)
        })
        .unwrap_or("other");
    let kind_type = normalize_faction_kind_type(kind_raw).unwrap_or_else(|_| "other".to_string());

    let mut kind_custom = parsed
        .as_ref()
        .and_then(|value| value.get("kind_custom").and_then(toml::Value::as_str))
        .map(normalize_unknown_text);
    if kind_type == "other" && kind_custom.is_none() {
        kind_custom = Some("Unknown".to_string());
    }
    if kind_type != "other" {
        kind_custom = None;
    }

    let parse_list_field = |field: &str| {
        parsed
            .as_ref()
            .and_then(|value| value.get(field))
            .and_then(|raw| {
                if let Some(items) = raw.as_array() {
                    let out: Vec<String> = items
                        .iter()
                        .filter_map(toml::Value::as_str)
                        .map(str::trim)
                        .filter(|item| !item.is_empty())
                        .map(ToString::to_string)
                        .collect();
                    return Some(out);
                }
                raw.as_str().map(parse_list_csv)
            })
            .unwrap_or_else(|| vec!["Unknown".to_string()])
    };

    db::FactionRow {
        id,
        slug,
        name,
        vault_path: normalize_relative_path_for_storage(relative_path),
        kind_type,
        kind_custom,
        public_description: parsed
            .as_ref()
            .and_then(|value| value.get("public_description").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        true_agenda: parsed
            .as_ref()
            .and_then(|value| value.get("true_agenda").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        methods: parsed
            .as_ref()
            .and_then(|value| value.get("methods").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        leadership: parsed
            .as_ref()
            .and_then(|value| value.get("leadership").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        headquarters: parsed
            .as_ref()
            .and_then(|value| value.get("headquarters").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        sphere_of_influence: parsed
            .as_ref()
            .and_then(|value| value.get("sphere_of_influence").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        resources_assets: parsed
            .as_ref()
            .and_then(|value| value.get("resources_assets").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        allies: serde_json::to_string(&normalize_unknown_list(parse_list_field("allies")))
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        rivals_enemies: serde_json::to_string(&normalize_unknown_list(parse_list_field("rivals_enemies")))
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        reputation: parsed
            .as_ref()
            .and_then(|value| value.get("reputation").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        current_tension: parsed
            .as_ref()
            .and_then(|value| value.get("current_tension").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        goals_short_term: serde_json::to_string(&normalize_unknown_list(parse_list_field("goals_short_term")))
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        goals_long_term: serde_json::to_string(&normalize_unknown_list(parse_list_field("goals_long_term")))
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        symbol_description: parsed
            .as_ref()
            .and_then(|value| value.get("symbol_description").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        created_at: parsed
            .as_ref()
            .and_then(|value| value.get("created_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| now.clone()),
        updated_at: parsed
            .as_ref()
            .and_then(|value| value.get("updated_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or(now),
    }
}

pub fn read_vault_file_if_exists(
    vault: &Vault,
    relative_path: &str,
) -> Result<Option<String>, String> {
    let relative = PathBuf::from(normalize_relative_path_for_storage(relative_path));
    let full = vault.resolve_relative(&relative).map_err(|err| err.to_string())?;
    if !full.exists() {
        return Ok(None);
    }

    std::fs::read_to_string(&full)
        .map(Some)
        .map_err(|err| format!("failed to read vault file {}: {}", full.display(), err))
}

pub fn unique_trash_path(
    vault: &Vault,
    entity_dir: &str,
    slug: &str,
    timestamp: &str,
) -> Result<String, String> {
    let base = format!("{}-{}", slug, timestamp.replace(':', "").replace('-', ""));
    let mut candidate = format!(".trash/{entity_dir}/{base}.md");
    let mut index = 2;

    loop {
        let full = vault
            .resolve_relative(&PathBuf::from(&candidate))
            .map_err(|err| err.to_string())?;
        if !full.exists() {
            return Ok(candidate);
        }
        candidate = format!(".trash/{entity_dir}/{base}-{index}.md");
        index += 1;
    }
}

pub fn move_vault_file(
    vault: &Vault,
    source_relative: &str,
    target_relative: &str,
) -> Result<(), String> {
    let source_relative = normalize_relative_path_for_storage(source_relative);
    let target_relative = normalize_relative_path_for_storage(target_relative);
    let source_full = vault
        .resolve_relative(&PathBuf::from(&source_relative))
        .map_err(|err| err.to_string())?;
    if !source_full.exists() {
        return Err(format!(
            "source file does not exist: {}",
            source_full.display()
        ));
    }

    let target_full = vault
        .resolve_relative(&PathBuf::from(&target_relative))
        .map_err(|err| err.to_string())?;
    if let Some(parent) = target_full.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create trash directory {}: {}", parent.display(), err))?;
    }

    fs::rename(&source_full, &target_full).map_err(|err| {
        format!(
            "failed to move file from {} to {}: {}",
            source_full.display(),
            target_full.display(),
            err
        )
    })
}

pub fn unique_markdown_path_for_name(
    vault: &Vault,
    relative_dir: &str,
    display_name: &str,
    keep_path: Option<&str>,
) -> Result<String, String> {
    let base = normalize_markdown_file_stem(display_name);
    let mut candidate = base.clone();
    let mut index = 2;

    loop {
        let relative = PathBuf::from(relative_dir)
            .join(format!("{candidate}.md"))
            .to_string_lossy()
            .to_string();
        let relative = normalize_relative_path_for_storage(&relative);

        if keep_path.is_some_and(|existing| existing == relative) {
            return Ok(relative);
        }

        let full = vault
            .resolve_relative(&PathBuf::from(&relative))
            .map_err(|err| err.to_string())?;
        if !full.exists() {
            return Ok(relative);
        }

        candidate = format!("{base} {index}");
        index += 1;
    }
}

fn extract_runebound_toml(contents: &str) -> Option<String> {
    let start = contents.find("```runebound")?;
    let mut body = &contents[start + "```runebound".len()..];
    if let Some(rest) = body.strip_prefix("\r\n") {
        body = rest;
    } else if let Some(rest) = body.strip_prefix('\n') {
        body = rest;
    }

    let end = body.find("\n```").or_else(|| body.find("```") )?;
    let block = body[..end].trim();
    if block.is_empty() {
        None
    } else {
        Some(block.to_string())
    }
}

fn stable_id_from_relative(prefix: &str, relative_path: &str) -> String {
    let suffix: String = relative_path
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("{prefix}_{suffix}")
}

fn file_stem_name(relative_path: &str) -> String {
    Path::new(relative_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Unknown")
        .to_string()
}

fn parse_list_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{extract_runebound_toml, scan_npc_row_from_markdown};

    #[test]
    fn extracts_runebound_toml_block() {
        let markdown = "# Note\n\n```runebound\ntype = \"npc\"\nname = \"Aelar\"\n```\n";
        let block = extract_runebound_toml(markdown).expect("expected runebound block");
        assert!(block.contains("type = \"npc\""));
        assert!(block.contains("name = \"Aelar\""));
    }

    #[test]
    fn indexes_npc_from_filename_without_runebound_block() {
        let row = scan_npc_row_from_markdown("npcs/Father Elen.md", "# Existing notes");
        assert_eq!(row.name, "Father Elen");
        assert_eq!(row.slug, "father-elen");
        assert_eq!(row.vault_path, "npcs/Father Elen.md");
    }
}
