use std::fs;
use std::path::PathBuf;

use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::entity_store::EntityStore;
use dnd_core::vault::Vault;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::app_state::{AppState, FactionDraftSession, ItemDraftSession, LocationDraftSession, NpcDraftSession};
use crate::commands::{ok_response, DesktopHandlerInvocation};
use crate::entities::EntityKind;
use crate::services::entity_admin::{EntityAdminService, EntityDetails, EntityType};
use crate::services::entity_persistence::{
    EntityPersistenceService, SaveFactionDraftInput, SaveItemDraftInput, SaveLocationDraftInput,
    SaveNpcDraftInput,
};
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
        match auto_save_active_draft(state).await? {
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

enum ActiveDraft {
    Npc(NpcDraftSession),
    Location(LocationDraftSession),
    Faction(FactionDraftSession),
    Item(ItemDraftSession),
}

async fn auto_save_active_draft(state: &AppState) -> Result<Option<PublishTargetInfo>, String> {
    let (kind, draft) = {
        let editor = state.editor_session.lock().await;
        let Some(kind) = editor.active_kind() else {
            return Ok(None);
        };
        let draft = match kind {
            EntityKind::Npc => editor.get_npc().cloned().map(ActiveDraft::Npc),
            EntityKind::Location => editor.get_location().cloned().map(ActiveDraft::Location),
            EntityKind::Faction => editor.get_faction().cloned().map(ActiveDraft::Faction),
            EntityKind::Item => editor.get_item().cloned().map(ActiveDraft::Item),
        };
        match draft {
            Some(draft) => (kind, draft),
            None => return Ok(None),
        }
    };

    let persistence = EntityPersistenceService;
    let publish_target = match (kind, draft) {
        (EntityKind::Npc, ActiveDraft::Npc(draft)) => {
            let result = persistence
                .save_npc_draft(
                    SaveNpcDraftInput {
                        id: draft.id.clone(),
                        name: draft.name.clone(),
                        race: draft.race.clone(),
                        occupation: draft.occupation.clone(),
                        sex: draft.sex.clone(),
                        age: draft.age.clone(),
                        height: draft.height.clone(),
                        weight_lbs: draft.weight_lbs.clone(),
                        background: draft.background.clone(),
                        want_need: draft.want_need.clone(),
                        secret_obstacle: draft.secret_obstacle.clone(),
                        carrying: draft.carrying.clone(),
                        location: draft.location.clone(),
                    },
                    state,
                )
                .await?;

            PublishTargetInfo {
                entity_type: EntityType::Npc,
                slug: result.slug,
                name: draft.name,
            }
        }
        (EntityKind::Location, ActiveDraft::Location(draft)) => {
            let result = persistence
                .save_location_draft(
                    SaveLocationDraftInput {
                        id: draft.id.clone(),
                        name: draft.name.clone(),
                        vault_path: draft.vault_path.clone(),
                        kind_type: draft.kind_type.clone(),
                        kind_custom: draft.kind_custom.clone(),
                        visual_description: draft.visual_description.clone(),
                        history_background: draft.history_background.clone(),
                        exports: draft.exports.clone(),
                        tone: draft.tone.clone(),
                        authority: draft.authority.clone(),
                        danger_level: draft.danger_level.clone(),
                        current_tension: draft.current_tension.clone(),
                    },
                    state,
                )
                .await?;

            PublishTargetInfo {
                entity_type: EntityType::Location,
                slug: result.slug,
                name: draft.name,
            }
        }
        (EntityKind::Faction, ActiveDraft::Faction(draft)) => {
            let result = persistence
                .save_faction_draft(
                    SaveFactionDraftInput {
                        id: draft.id.clone(),
                        name: draft.name.clone(),
                        vault_path: draft.vault_path.clone(),
                        kind_type: draft.kind_type.clone(),
                        kind_custom: draft.kind_custom.clone(),
                        public_description: draft.public_description.clone(),
                        true_agenda: draft.true_agenda.clone(),
                        methods: draft.methods.clone(),
                        leadership: draft.leadership.clone(),
                        headquarters: draft.headquarters.clone(),
                        sphere_of_influence: draft.sphere_of_influence.clone(),
                        resources_assets: draft.resources_assets.clone(),
                        allies: draft.allies.clone(),
                        rivals_enemies: draft.rivals_enemies.clone(),
                        reputation: draft.reputation.clone(),
                        current_tension: draft.current_tension.clone(),
                        goals_short_term: draft.goals_short_term.clone(),
                        goals_long_term: draft.goals_long_term.clone(),
                        symbol_description: draft.symbol_description.clone(),
                    },
                    state,
                )
                .await?;

            PublishTargetInfo {
                entity_type: EntityType::Faction,
                slug: result.slug,
                name: draft.name,
            }
        }
        (EntityKind::Item, ActiveDraft::Item(draft)) => {
            let result = persistence
                .save_item_draft(
                    SaveItemDraftInput {
                        id: draft.id.clone(),
                        name: draft.name.clone(),
                        category: draft.category.clone(),
                        rarity: draft.rarity.clone(),
                        attunement: draft.attunement.clone(),
                        materials: draft.materials.clone(),
                        appearance: draft.appearance.clone(),
                        abilities: draft.abilities.clone(),
                        drawbacks: draft.drawbacks.clone(),
                        history: draft.history.clone(),
                        value: draft.value.clone(),
                        location: draft.location.clone(),
                    },
                    state,
                )
                .await?;

            PublishTargetInfo {
                entity_type: EntityType::Item,
                slug: result.slug,
                name: draft.name,
            }
        }
        _ => return Ok(None),
    };

    {
        let mut editor = state.editor_session.lock().await;
        editor.clear_all();
    }

    Ok(Some(publish_target))
}
