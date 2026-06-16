use crate::app_state::AppState;
use crate::commands::{ok_response, ok_response_with_doc, DesktopHandlerInvocation};
use dnd_core::command::CommandClientEvent;
use runebound_models::{CommandResponse, OutputDoc, entity_card, entity_row};

use crate::services::entity_admin::{
    EntityAdminService, EntityDetails, EntityType, SoftDeleteEntityInput,
};
use crate::entities::EntityKind;
use crate::entities::domains::{
    event_event_from_draft,
    faction_event_from_draft,
    god_event_from_draft,
    item_event_from_draft,
    location_event_from_draft,
    npc_event_from_draft,
};
use crate::utils::path_for_display;
use crate::app_state::{
    EventDraftSession, FactionDraftSession, GodDraftSession, ItemDraftSession, LocationDraftSession,
    NpcDraftSession,
};

pub async fn handle_load(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "load" {
        return Ok(Some(ok_response("usage: load <npc-or-location-or-faction-name>".to_string(), None)));
    }
    if !lowered.starts_with("load ") {
        return Ok(None);
    }

    let target = trimmed[4..].trim();
    if target.is_empty() {
        return Ok(Some(ok_response("usage: load <npc-or-location-or-faction-name>".to_string(), None)));
    }

    let admin = EntityAdminService;
    let entity = admin
        .resolve_entity(target.to_string(), invocation.state.inner())
        .await?;
    let Some(entity) = entity else {
        return Ok(Some(ok_response(format!("no npc, location, or faction found for: {target}"), None)));
    };

    let (output, event) = build_load_response(entity, invocation.state.clone()).await;
    Ok(Some(ok_response(output, event)))
}

pub async fn handle_show(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    entity_preview_response(invocation, "show").await
}

pub async fn handle_preview(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    entity_preview_response(invocation, "preview").await
}

async fn entity_preview_response(
    invocation: DesktopHandlerInvocation<'_>,
    root: &str,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    if lowered == root {
        return Ok(Some(ok_response(format!("usage: {} <npc-or-location-or-faction-name>", root), None)));
    }
    if !lowered.starts_with(&format!("{root} ")) {
        return Ok(None);
    }
    let target = trimmed[root.len()..].trim();
    if target.is_empty() {
        return Ok(Some(ok_response(format!("usage: {} <npc-or-location-or-faction-name>", root), None)));
    }
    let admin = EntityAdminService;
    let entity = admin
        .resolve_entity(target.to_string(), invocation.state.inner())
        .await?;
    let Some(entity) = entity else {
        return Ok(Some(ok_response(format!("no npc, location, or faction found for: {target}"), None)));
    };

    let preview_text = build_preview_response(entity.clone());
    let preview_doc = build_entity_card_doc(&entity);
    Ok(Some(ok_response_with_doc(preview_text, Some(preview_doc), None)))
}

pub async fn handle_delete(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    if lowered == "delete" {
        return Ok(Some(ok_response("usage: delete <npc-or-location-or-faction-name>".to_string(), None)));
    }
    if !lowered.starts_with("delete ") {
        return Ok(None);
    }
    let target = trimmed[6..].trim();
    if target.is_empty() {
        return Ok(Some(ok_response("usage: delete <npc-or-location-or-faction-name>".to_string(), None)));
    }

    let admin = EntityAdminService;
    let result = admin
        .soft_delete_entity(SoftDeleteEntityInput { target: target.to_string() }, invocation.state.inner())
        .await?;

    let output = [
        "## Deleted".to_string(),
        format!("type: {}", result.entity_type.as_str()),
        format!("name: {}", result.name),
        format!("slug: {}", result.slug),
        format!("trash: {}", path_for_display(&result.trash_vault_path)),
        "tip: run undo to restore it.".to_string(),
    ].join("\n");

    let should_clear = {
        let editor = invocation.state.editor_session.lock().await;
        editor
            .get_npc()
            .is_some_and(|draft| draft.id == result.id)
            || editor
                .get_location()
                .is_some_and(|draft| draft.id == result.id)
            || editor
                .get_faction()
                .is_some_and(|draft| draft.id == result.id)
            || editor
                .get_god()
                .is_some_and(|draft| draft.id == result.id)
    };

    if should_clear {
        let mut editor = invocation.state.editor_session.lock().await;
        editor.clear_all();
        return Ok(Some(ok_response(output, Some(CommandClientEvent::ClearDrafts))));
    }

    Ok(Some(ok_response(output, None)))
}

pub async fn handle_undo(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let admin = EntityAdminService;
    let result = admin.undo_last_soft_delete(invocation.state.inner()).await?;
    let output = [
        "## Undo complete".to_string(),
        format!("type: {}", result.entity_type.as_str()),
        format!("name: {}", result.name),
        format!("slug: {}", result.slug),
        format!("vault: {}", path_for_display(&result.vault_path)),
    ].join("\n");
    Ok(Some(ok_response(output, None)))
}

pub(crate) async fn build_load_response(entity: EntityDetails, state: tauri::State<'_, AppState>) -> (String, Option<CommandClientEvent>) {
    match entity.entity_type {
        EntityType::Npc => {
            let draft = NpcDraftSession {
                id: entity.id.clone(),
                seed_prompt: None,
                name: entity.name.clone(),
                slug: entity.slug.clone(),
                race: entity.race.clone().unwrap_or_else(|| "Unknown".to_string()),
                occupation: entity.occupation.clone().unwrap_or_else(|| "Unknown".to_string()),
                sex: normalize_sex(&entity.sex.clone().unwrap_or_else(|| "male".to_string())).unwrap_or_else(|_| "male".to_string()),
                age: entity.age.clone().unwrap_or_else(|| "Unknown".to_string()),
                height: entity.height.clone().unwrap_or_else(|| "Unknown".to_string()),
                weight_lbs: entity.weight_lbs.clone().unwrap_or_else(|| "Unknown".to_string()),
                background: entity.background.clone().unwrap_or_else(|| "Unknown".to_string()),
                want_need: entity.want_need.clone().unwrap_or_else(|| "Unknown".to_string()),
                secret_obstacle: entity.secret_obstacle.clone().unwrap_or_else(|| "Unknown".to_string()),
                carrying: entity.carrying.clone().unwrap_or_else(|| vec!["Unknown".to_string()]),
                location: entity.location.clone().unwrap_or_else(|| "Unknown".to_string()),
            };
            {
                let mut editor = state.editor_session.lock().await;
                editor.set_npc(draft.clone());
                editor.clear_kind(EntityKind::Location);
            }
            (build_entity_card_text(&entity), Some(npc_event_from_draft(&draft)))
        }
        EntityType::Location => {
            let draft = LocationDraftSession {
                id: entity.id.clone(),
                seed_prompt: None,
                name: entity.name.clone(),
                slug: entity.slug.clone(),
                vault_path: path_for_display(&entity.vault_path),
                kind_type: entity.kind_type.clone().unwrap_or_else(|| "other".to_string()),
                kind_custom: entity.kind_custom.clone(),
                visual_description: entity.visual_description.clone().unwrap_or_else(|| "Unknown".to_string()),
                history_background: entity.history_background.clone().unwrap_or_else(|| "Unknown".to_string()),
                exports: entity.exports.clone().unwrap_or_else(|| vec!["Unknown".to_string()]),
                tone: entity.tone.clone().unwrap_or_else(|| "Unknown".to_string()),
                authority: entity.authority.clone().unwrap_or_else(|| "Unknown".to_string()),
                danger_level: entity.danger_level.clone().unwrap_or_else(|| "Unknown".to_string()),
                current_tension: entity.current_tension.clone().unwrap_or_else(|| "Unknown".to_string()),
            };
            {
                let mut editor = state.editor_session.lock().await;
                editor.set_location(draft.clone());
                editor.clear_kind(EntityKind::Npc);
            }
            (build_entity_card_text(&entity), Some(location_event_from_draft(&draft)))
        }
        EntityType::Faction => {
            let draft = FactionDraftSession {
                id: entity.id.clone(),
                seed_prompt: None,
                name: entity.name.clone(),
                slug: entity.slug.clone(),
                vault_path: path_for_display(&entity.vault_path),
                kind_type: entity.kind_type.clone().unwrap_or_else(|| "other".to_string()),
                kind_custom: entity.kind_custom.clone(),
                public_description: entity.public_description.clone().unwrap_or_else(|| "Unknown".to_string()),
                true_agenda: entity.true_agenda.clone().unwrap_or_else(|| "Unknown".to_string()),
                methods: entity.methods.clone().unwrap_or_else(|| "Unknown".to_string()),
                leadership: entity.leadership.clone().unwrap_or_else(|| "Unknown".to_string()),
                headquarters: entity.headquarters.clone().unwrap_or_else(|| "Unknown".to_string()),
                sphere_of_influence: entity.sphere_of_influence.clone().unwrap_or_else(|| "Unknown".to_string()),
                resources_assets: entity.resources_assets.clone().unwrap_or_else(|| "Unknown".to_string()),
                allies: entity.allies.clone().unwrap_or_else(|| vec!["Unknown".to_string()]),
                rivals_enemies: entity.rivals_enemies.clone().unwrap_or_else(|| vec!["Unknown".to_string()]),
                reputation: entity.reputation.clone().unwrap_or_else(|| "Unknown".to_string()),
                current_tension: entity.current_tension.clone().unwrap_or_else(|| "Unknown".to_string()),
                goals_short_term: entity.goals_short_term.clone().unwrap_or_else(|| vec!["Unknown".to_string()]),
                goals_long_term: entity.goals_long_term.clone().unwrap_or_else(|| vec!["Unknown".to_string()]),
                symbol_description: entity.symbol_description.clone().unwrap_or_else(|| "Unknown".to_string()),
            };
            {
                let mut editor = state.editor_session.lock().await;
                editor.set_faction(draft.clone());
                editor.clear_kind(EntityKind::Npc);
                editor.clear_kind(EntityKind::Location);
            }
            (build_entity_card_text(&entity), Some(faction_event_from_draft(&draft)))
        }
        EntityType::Item => {
            let draft = ItemDraftSession {
                id: entity.id.clone(),
                seed_prompt: None,
                name: entity.name.clone(),
                slug: entity.slug.clone(),
                vault_path: path_for_display(&entity.vault_path),
                category: entity.category.clone().unwrap_or_else(|| "other".to_string()),
                rarity: entity.rarity.clone().unwrap_or_else(|| "unknown".to_string()),
                attunement: entity.attunement.clone().unwrap_or_else(|| "Unknown".to_string()),
                materials: entity.materials.clone().unwrap_or_else(|| vec!["Unknown".to_string()]),
                appearance: entity.appearance.clone().unwrap_or_else(|| "Unknown".to_string()),
                abilities: entity.abilities.clone().unwrap_or_else(|| "Unknown".to_string()),
                drawbacks: entity.drawbacks.clone().unwrap_or_else(|| "Unknown".to_string()),
                history: entity.history.clone().unwrap_or_else(|| "Unknown".to_string()),
                value: entity.value.clone().unwrap_or_else(|| "Unknown".to_string()),
                location: entity.location.clone().unwrap_or_else(|| "Unknown".to_string()),
            };
            {
                let mut editor = state.editor_session.lock().await;
                editor.set_item(draft.clone());
                editor.clear_kind(EntityKind::Npc);
                editor.clear_kind(EntityKind::Location);
                editor.clear_kind(EntityKind::Faction);
            }
            (build_entity_card_text(&entity), Some(item_event_from_draft(&draft)))
        }
        EntityType::Event => {
            let draft = EventDraftSession {
                id: entity.id.clone(),
                seed_prompt: None,
                name: entity.name.clone(),
                slug: entity.slug.clone(),
                body: entity.body.clone().unwrap_or_default(),
            };
            {
                let mut editor = state.editor_session.lock().await;
                editor.set_event(draft.clone());
                editor.clear_kind(EntityKind::Npc);
                editor.clear_kind(EntityKind::Location);
                editor.clear_kind(EntityKind::Faction);
                editor.clear_kind(EntityKind::Item);
            }
            (build_entity_card_text(&entity), Some(event_event_from_draft(&draft)))
        }
        EntityType::God => {
            let draft = GodDraftSession {
                id: entity.id.clone(),
                seed_prompt: None,
                name: entity.name.clone(),
                slug: entity.slug.clone(),
                vault_path: path_for_display(&entity.vault_path),
                epithet: entity.epithet.clone().unwrap_or_else(|| "Unknown".to_string()),
                rank: entity.rank.clone().unwrap_or_else(|| "other".to_string()),
                rank_custom: entity.rank_custom.clone(),
                alignment: entity.alignment.clone().unwrap_or_else(|| "TN".to_string()),
                domains: entity.domains.clone().unwrap_or_else(|| vec!["Unknown".to_string()]),
                symbol: entity.symbol.clone().unwrap_or_else(|| "Unknown".to_string()),
                appearance: entity.appearance.clone().unwrap_or_else(|| "Unknown".to_string()),
                dogma: entity.dogma.clone().unwrap_or_else(|| "Unknown".to_string()),
                realm: entity.realm.clone().unwrap_or_else(|| "Unknown".to_string()),
                worshippers: entity.worshippers.clone().unwrap_or_else(|| "Unknown".to_string()),
                clergy: entity.clergy.clone().unwrap_or_else(|| "Unknown".to_string()),
                allies: entity.allies.clone().unwrap_or_else(|| vec!["Unknown".to_string()]),
                rivals: entity.rivals.clone().unwrap_or_else(|| vec!["Unknown".to_string()]),
            };
            {
                let mut editor = state.editor_session.lock().await;
                editor.set_god(draft.clone());
                editor.clear_kind(EntityKind::Npc);
                editor.clear_kind(EntityKind::Location);
                editor.clear_kind(EntityKind::Faction);
                editor.clear_kind(EntityKind::Item);
                editor.clear_kind(EntityKind::Event);
            }
            (build_entity_card_text(&entity), Some(god_event_from_draft(&draft)))
        }
    }
}

fn build_preview_response(entity: EntityDetails) -> String {
    build_entity_card_text(&entity)
}

fn build_entity_card_doc(entity: &EntityDetails) -> OutputDoc {
    let mut rows = vec![
        entity_row("name", entity.name.clone()),
        entity_row("slug", entity.slug.clone()),
    ];

    match entity.entity_type {
        EntityType::Npc => {
            rows.push(entity_row("race", entity.race.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("occupation", entity.occupation.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("sex", entity.sex.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("age", entity.age.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("height", entity.height.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("weight", entity.weight_lbs.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("background", entity.background.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("want", entity.want_need.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("secret", entity.secret_obstacle.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("carrying", entity.carrying.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", ")));
            rows.push(entity_row("location", entity.location.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("path", path_for_display(&entity.vault_path)));
            OutputDoc { blocks: vec![entity_card("NPC", rows)] }
        }
        EntityType::Location => {
            rows.push(entity_row("kind", entity.kind_type.clone().unwrap_or_else(|| "other".to_string())));
            rows.push(entity_row("kind_custom", entity.kind_custom.clone().unwrap_or_else(|| "(none)".to_string())));
            rows.push(entity_row("visual", entity.visual_description.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("history", entity.history_background.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("exports", entity.exports.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", ")));
            rows.push(entity_row("tone", entity.tone.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("authority", entity.authority.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("danger", entity.danger_level.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("tension", entity.current_tension.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("path", path_for_display(&entity.vault_path)));
            OutputDoc { blocks: vec![entity_card("Location", rows)] }
        }
        EntityType::Faction => {
            rows.push(entity_row("kind", entity.kind_type.clone().unwrap_or_else(|| "other".to_string())));
            rows.push(entity_row("kind_custom", entity.kind_custom.clone().unwrap_or_else(|| "(none)".to_string())));
            rows.push(entity_row("public", entity.public_description.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("agenda", entity.true_agenda.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("methods", entity.methods.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("leadership", entity.leadership.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("headquarters", entity.headquarters.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("influence", entity.sphere_of_influence.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("resources", entity.resources_assets.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("allies", entity.allies.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", ")));
            rows.push(entity_row("rivals", entity.rivals_enemies.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", ")));
            rows.push(entity_row("reputation", entity.reputation.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("tension", entity.current_tension.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("goals_short", entity.goals_short_term.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", ")));
            rows.push(entity_row("goals_long", entity.goals_long_term.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", ")));
            rows.push(entity_row("symbol", entity.symbol_description.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("path", path_for_display(&entity.vault_path)));
            OutputDoc { blocks: vec![entity_card("Faction", rows)] }
        }
        EntityType::Item => {
            rows.push(entity_row("category", entity.category.clone().unwrap_or_else(|| "other".to_string())));
            rows.push(entity_row("rarity", entity.rarity.clone().unwrap_or_else(|| "unknown".to_string())));
            rows.push(entity_row("attunement", entity.attunement.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row(
                "materials",
                entity
                    .materials
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()])
                    .join(", "),
            ));
            rows.push(entity_row("appearance", entity.appearance.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("abilities", entity.abilities.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("drawbacks", entity.drawbacks.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("history", entity.history.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("value", entity.value.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("location", entity.location.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("path", path_for_display(&entity.vault_path)));
            OutputDoc { blocks: vec![entity_card("Item", rows)] }
        }
        EntityType::Event => {
            rows.push(entity_row("body", entity.body.clone().unwrap_or_default()));
            rows.push(entity_row("path", path_for_display(&entity.vault_path)));
            OutputDoc { blocks: vec![entity_card("Event", rows)] }
        }
        EntityType::God => {
            rows.push(entity_row("epithet", entity.epithet.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("rank", entity.rank.clone().unwrap_or_else(|| "other".to_string())));
            rows.push(entity_row("rank_custom", entity.rank_custom.clone().unwrap_or_else(|| "(none)".to_string())));
            rows.push(entity_row("alignment", entity.alignment.clone().unwrap_or_else(|| "TN".to_string())));
            rows.push(entity_row("domains", entity.domains.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", ")));
            rows.push(entity_row("symbol", entity.symbol.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("appearance", entity.appearance.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("dogma", entity.dogma.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("realm", entity.realm.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("worshippers", entity.worshippers.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("clergy", entity.clergy.clone().unwrap_or_else(|| "Unknown".to_string())));
            rows.push(entity_row("allies", entity.allies.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", ")));
            rows.push(entity_row("rivals", entity.rivals.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", ")));
            rows.push(entity_row("path", path_for_display(&entity.vault_path)));
            OutputDoc { blocks: vec![entity_card("God", rows)] }
        }
    }
}

fn build_entity_card_text(entity: &EntityDetails) -> String {
    match entity.entity_type {
        EntityType::Npc => {
            let carrying = entity.carrying.as_ref().map(|items| items.join(", ")).unwrap_or_else(|| "Unknown".to_string());
            format!(
                "## NPC\nname: {}\nslug: {}\nrace: {}\noccupation: {}\nsex: {}\nage: {}\nheight: {}\nweight: {}\nbackground: {}\nwant: {}\nsecret: {}\ncarrying: {}\nlocation: {}\npath: {}",
                entity.name, entity.slug,
                entity.race.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.occupation.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.sex.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.age.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.height.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.weight_lbs.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.background.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.want_need.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.secret_obstacle.clone().unwrap_or_else(|| "Unknown".to_string()),
                carrying,
                entity.location.clone().unwrap_or_else(|| "Unknown".to_string()),
                path_for_display(&entity.vault_path)
            )
        }
        EntityType::Location => {
            format!(
                "## Location\nname: {}\nslug: {}\nkind: {}\nkind_custom: {}\nvisual: {}\nhistory: {}\nexports: {}\ntone: {}\nauthority: {}\ndanger: {}\ntension: {}\npath: {}",
                entity.name, entity.slug,
                entity.kind_type.clone().unwrap_or_else(|| "other".to_string()),
                entity.kind_custom.clone().unwrap_or_else(|| "(none)".to_string()),
                entity.visual_description.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.history_background.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.exports.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", "),
                entity.tone.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.authority.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.danger_level.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.current_tension.clone().unwrap_or_else(|| "Unknown".to_string()),
                path_for_display(&entity.vault_path)
            )
        }
        EntityType::Faction => {
            format!(
                "## Faction\nname: {}\nslug: {}\nkind: {}\nkind_custom: {}\npublic: {}\nagenda: {}\nmethods: {}\nleadership: {}\nheadquarters: {}\ninfluence: {}\nresources: {}\nallies: {}\nrivals: {}\nreputation: {}\ntension: {}\ngoals_short: {}\ngoals_long: {}\nsymbol: {}\npath: {}",
                entity.name, entity.slug,
                entity.kind_type.clone().unwrap_or_else(|| "other".to_string()),
                entity.kind_custom.clone().unwrap_or_else(|| "(none)".to_string()),
                entity.public_description.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.true_agenda.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.methods.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.leadership.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.headquarters.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.sphere_of_influence.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.resources_assets.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.allies.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", "),
                entity.rivals_enemies.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", "),
                entity.reputation.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.current_tension.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.goals_short_term.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", "),
                entity.goals_long_term.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", "),
                entity.symbol_description.clone().unwrap_or_else(|| "Unknown".to_string()),
                path_for_display(&entity.vault_path)
            )
        }
        EntityType::Item => {
            let materials = entity
                .materials
                .as_ref()
                .map(|items| items.join(", "))
                .unwrap_or_else(|| "Unknown".to_string());
            format!(
                "## Item\nname: {}\nslug: {}\ncategory: {}\nrarity: {}\nattunement: {}\nmaterials: {}\nappearance: {}\nabilities: {}\ndrawbacks: {}\nhistory: {}\nvalue: {}\nlocation: {}\npath: {}",
                entity.name,
                entity.slug,
                entity.category.clone().unwrap_or_else(|| "other".to_string()),
                entity.rarity.clone().unwrap_or_else(|| "unknown".to_string()),
                entity.attunement.clone().unwrap_or_else(|| "Unknown".to_string()),
                materials,
                entity.appearance.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.abilities.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.drawbacks.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.history.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.value.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.location.clone().unwrap_or_else(|| "Unknown".to_string()),
                path_for_display(&entity.vault_path)
            )
        }
        EntityType::Event => {
            format!(
                "## Event\nname: {}\nslug: {}\npath: {}\n\n{}",
                entity.name,
                entity.slug,
                path_for_display(&entity.vault_path),
                entity.body.clone().unwrap_or_default(),
            )
        }
        EntityType::God => {
            format!(
                "## God\nname: {}\nslug: {}\nepithet: {}\nrank: {}\nrank_custom: {}\nalignment: {}\ndomains: {}\nsymbol: {}\nappearance: {}\ndogma: {}\nrealm: {}\nworshippers: {}\nclergy: {}\nallies: {}\nrivals: {}\npath: {}",
                entity.name, entity.slug,
                entity.epithet.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.rank.clone().unwrap_or_else(|| "other".to_string()),
                entity.rank_custom.clone().unwrap_or_else(|| "(none)".to_string()),
                entity.alignment.clone().unwrap_or_else(|| "TN".to_string()),
                entity.domains.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", "),
                entity.symbol.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.appearance.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.dogma.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.realm.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.worshippers.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.clergy.clone().unwrap_or_else(|| "Unknown".to_string()),
                entity.allies.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", "),
                entity.rivals.clone().unwrap_or_else(|| vec!["Unknown".to_string()]).join(", "),
                path_for_display(&entity.vault_path)
            )
        }
    }
}

fn normalize_sex(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "male" || normalized == "female" { Ok(normalized) }
    else { Err("sex must be one of: male, female".to_string()) }
}
