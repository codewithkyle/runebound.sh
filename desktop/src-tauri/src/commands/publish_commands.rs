use std::fs;
use std::path::PathBuf;

use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::entity_store::EntityStore;
use dnd_core::vault::Vault;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::commands::{ok_response, DesktopHandlerInvocation};
use crate::services::entity_admin::{EntityAdminService, EntityType};
use crate::services::publish::{
    render_faction_markdown, render_item_markdown, render_location_markdown, render_npc_markdown,
};
use runebound_models::CommandResponse;

pub type CommandResult = Result<Option<CommandResponse>, String>;

pub async fn handle_publish(
    invocation: DesktopHandlerInvocation<'_>,
) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "publish help" {
        return publish_help();
    }

    if !lowered.starts_with("publish ") {
        return Ok(Some(ok_response(
            "usage: publish <entity name or slug>".to_string(),
            None,
        )));
    }

    let target = trimmed["publish".len()..].trim();
    if target.is_empty() {
        return Ok(Some(ok_response(
            "usage: publish <entity name or slug>".to_string(),
            None,
        )));
    }

    let state = invocation.state.inner();
    let admin = EntityAdminService;
    let Some(details) = admin
        .resolve_entity(target.to_string(), state)
        .await?
    else {
        return Ok(Some(ok_response(
            format!("no npc, location, faction, or item found for '{target}'"),
            None,
        )));
    };

    let store = EntityStore::new(&state.workspace_root).map_err(|err| err.to_string())?;
    let (markdown, vault_path) = match details.entity_type {
        EntityType::Npc => {
            let frontmatter = store
                .load_npc(&details.slug)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| format!("canonical record for '{}' is missing", details.slug))?;
            (render_npc_markdown(&frontmatter), frontmatter.vault_path)
        }
        EntityType::Location => {
            let frontmatter = store
                .load_location(&details.slug)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| format!("canonical record for '{}' is missing", details.slug))?;
            (render_location_markdown(&frontmatter), frontmatter.vault_path)
        }
        EntityType::Faction => {
            let frontmatter = store
                .load_faction(&details.slug)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| format!("canonical record for '{}' is missing", details.slug))?;
            (render_faction_markdown(&frontmatter), frontmatter.vault_path)
        }
        EntityType::Item => {
            let frontmatter = store
                .load_item(&details.slug)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| format!("canonical record for '{}' is missing", details.slug))?;
            (render_item_markdown(&frontmatter), frontmatter.vault_path)
        }
    };

    let effective = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&effective.effective).map_err(|err| err.to_string())?;
    let vault_root = effective
        .effective
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured; run start setup".to_string())?;
    let vault = Vault::new(vault_root);
    state.vault_repo().ensure_structure(&vault)?;

    let relative = PathBuf::from(&vault_path);
    let full_path = vault
        .resolve_relative(&relative)
        .map_err(|err| err.to_string())?;

    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create directory {}: {err}", parent.display()))?;
    }

    let should_write = if full_path.exists() {
        invocation
            .app_handle
            .dialog()
            .message(format!(
                "{} already exists. Overwrite?",
                relative.display()
            ))
            .title("Overwrite file?")
            .buttons(MessageDialogButtons::YesNo)
            .blocking_show()
    } else {
        true
    };

    if !should_write {
        return Ok(Some(ok_response(
            "Publish cancelled; file was left untouched.".to_string(),
            None,
        )));
    }

    fs::write(&full_path, markdown)
        .map_err(|err| format!("failed to write {}: {err}", full_path.display()))?;

    Ok(Some(ok_response(
        format!("Published {} to {}", details.name, relative.display()),
        None,
    )))
}

fn publish_help() -> CommandResult {
    let text = "Publish an entity's canonical record to your Obsidian vault.\n\nUsage:\n  publish <name or slug>\n\nPublishing overwrites the target markdown file. If it already exists you will be asked to confirm before it is replaced.";
    Ok(Some(ok_response(text.to_string(), None)))
}
