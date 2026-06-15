use std::fs;
use std::path::PathBuf;

use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::entity_store::EntityStore;
use dnd_core::vault::Vault;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::app_state::AppState;
use crate::commands::{ok_response, DesktopHandlerInvocation};
use crate::entities::EntityKind;
use crate::services::entity_admin::{EntityAdminService, EntityDetails, EntityType};
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

    if !lowered.starts_with("publish") {
        return Ok(Some(ok_response(
            "usage: publish [entity name or slug]".to_string(),
            None,
        )));
    }

    let state = invocation.state.inner();
    let args = if trimmed.len() > "publish".len() {
        trimmed["publish".len()..].trim()
    } else {
        ""
    };

    let target = if args.is_empty() {
        match active_draft_target(state).await? {
            Some(target) => target,
            None => {
                return Ok(Some(ok_response(
                    "No active draft to publish. Provide a name (e.g., `publish Lirael`) or load an entity first.".to_string(),
                    None,
                )))
            }
        }
    } else if lowered == "publish help" {
        // Already handled above, but keep guard for safety.
        return publish_help();
    } else {
        let admin = EntityAdminService;
        let Some(details) = admin
            .resolve_entity(args.to_string(), state)
            .await?
        else {
            return Ok(Some(ok_response(
                format!("no npc, location, faction, or item found for '{args}'"),
                None,
            )));
        };
        PublishTargetInfo::from_details(details)
    };

    let store = EntityStore::new(&state.workspace_root).map_err(|err| err.to_string())?;
    let (markdown, vault_path) = match target.entity_type {
        EntityType::Npc => {
            let frontmatter = match store
                .load_npc(&target.slug)
                .map_err(|err| err.to_string())?
            {
                Some(data) => data,
                None => return Ok(Some(ok_response(missing_canonical_message(&target), None))),
            };
            (render_npc_markdown(&frontmatter), frontmatter.vault_path)
        }
        EntityType::Location => {
            let frontmatter = match store
                .load_location(&target.slug)
                .map_err(|err| err.to_string())?
            {
                Some(data) => data,
                None => return Ok(Some(ok_response(missing_canonical_message(&target), None))),
            };
            (render_location_markdown(&frontmatter), frontmatter.vault_path)
        }
        EntityType::Faction => {
            let frontmatter = match store
                .load_faction(&target.slug)
                .map_err(|err| err.to_string())?
            {
                Some(data) => data,
                None => return Ok(Some(ok_response(missing_canonical_message(&target), None))),
            };
            (render_faction_markdown(&frontmatter), frontmatter.vault_path)
        }
        EntityType::Item => {
            let frontmatter = match store
                .load_item(&target.slug)
                .map_err(|err| err.to_string())?
            {
                Some(data) => data,
                None => return Ok(Some(ok_response(missing_canonical_message(&target), None))),
            };
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
        format!("Published {} to {}", target.name, relative.display()),
        None,
    )))
}

fn publish_help() -> CommandResult {
    let text = "Publish an entity's canonical record to your Obsidian vault.\n\nUsage:\n  publish\n  publish <name or slug>\n\nIf you omit a name while editing an entity, the active draft is published. Publishing overwrites the target markdown file. If it already exists you will be asked to confirm before it is replaced.";
    Ok(Some(ok_response(text.to_string(), None)))
}

struct PublishTargetInfo {
    entity_type: EntityType,
    slug: String,
    name: String,
}

impl PublishTargetInfo {
    fn from_details(details: EntityDetails) -> Self {
        Self {
            entity_type: details.entity_type,
            slug: details.slug,
            name: details.name,
        }
    }
}

async fn active_draft_target(state: &AppState) -> Result<Option<PublishTargetInfo>, String> {
    let editor = state.editor_session.lock().await;
    let kind = match editor.active_kind() {
        Some(kind) => kind,
        None => return Ok(None),
    };

    let info = match kind {
        EntityKind::Npc => editor.get_npc().map(|draft| PublishTargetInfo {
            entity_type: EntityType::Npc,
            slug: draft.slug.clone(),
            name: draft.name.clone(),
        }),
        EntityKind::Location => editor.get_location().map(|draft| PublishTargetInfo {
            entity_type: EntityType::Location,
            slug: draft.slug.clone(),
            name: draft.name.clone(),
        }),
        EntityKind::Faction => editor.get_faction().map(|draft| PublishTargetInfo {
            entity_type: EntityType::Faction,
            slug: draft.slug.clone(),
            name: draft.name.clone(),
        }),
        EntityKind::Item => editor.get_item().map(|draft| PublishTargetInfo {
            entity_type: EntityType::Item,
            slug: draft.slug.clone(),
            name: draft.name.clone(),
        }),
    };

    Ok(info)
}

fn missing_canonical_message(target: &PublishTargetInfo) -> String {
    format!(
        "{} has not been saved yet. Run `{} save` before publishing.",
        target.name,
        command_root_for(&target.entity_type)
    )
}

fn command_root_for(entity_type: &EntityType) -> &'static str {
    match entity_type {
        EntityType::Npc => "npc",
        EntityType::Location => "location",
        EntityType::Faction => "faction",
        EntityType::Item => "item",
    }
}
