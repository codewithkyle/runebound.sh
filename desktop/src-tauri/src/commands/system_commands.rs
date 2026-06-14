use crate::app_state::{AppState, EditorMode};
use crate::commands::{ok_response, DesktopHandlerInvocation};
use crate::services::ai_generation::AiGenerationService;
use dnd_core::command::CommandClientEvent;
use runebound_models::CommandResponse;


pub async fn handle_save(invocation: DesktopHandlerInvocation<'_>) -> Result<Option<CommandResponse>, String> {
    let mode = {
        let editor = invocation.state.editor_session.lock().await;
        editor.mode
    };

    match mode {
        EditorMode::Npc => npc_save(invocation.state.clone()).await,
        EditorMode::Location => location_save(invocation.state.clone()).await,
        EditorMode::Faction => faction_save(invocation.state.clone()).await,
        EditorMode::None => Ok(Some(ok_response("no active draft to save.".to_string(), None))),
    }
}

pub async fn handle_reroll(invocation: DesktopHandlerInvocation<'_>) -> Result<Option<CommandResponse>, String> {
    let mode = {
        let editor = invocation.state.editor_session.lock().await;
        editor.mode
    };

    match mode {
        EditorMode::Npc => reroll_current_npc(invocation.state.clone()).await,
        EditorMode::Location => reroll_current_location(invocation.state.clone()).await,
        EditorMode::Faction => reroll_current_faction(invocation.state.clone()).await,
        EditorMode::None => Ok(Some(ok_response("no active draft to reroll.".to_string(), None))),
    }
}

pub async fn handle_cancel(invocation: DesktopHandlerInvocation<'_>) -> Result<Option<CommandResponse>, String> {
    let mut editor = invocation.state.editor_session.lock().await;
    let response = match editor.mode {
        EditorMode::Npc => {
            if editor.npc_draft.is_none() {
                ok_response("no active npc draft.".to_string(), None)
            } else {
                editor.npc_draft = None;
                editor.mode = if editor.location_draft.is_some() {
                    EditorMode::Location
                } else if editor.faction_draft.is_some() {
                    EditorMode::Faction
                } else {
                    EditorMode::None
                };
                ok_response("npc draft discarded.".to_string(), Some(CommandClientEvent::ClearDrafts))
            }
        }
        EditorMode::Location => {
            if editor.location_draft.is_none() {
                ok_response("no active location draft.".to_string(), None)
            } else {
                editor.location_draft = None;
                editor.mode = if editor.npc_draft.is_some() {
                    EditorMode::Npc
                } else if editor.faction_draft.is_some() {
                    EditorMode::Faction
                } else {
                    EditorMode::None
                };
                ok_response("location draft discarded.".to_string(), Some(CommandClientEvent::ClearDrafts))
            }
        }
        EditorMode::Faction => {
            if editor.faction_draft.is_none() {
                ok_response("no active faction draft.".to_string(), None)
            } else {
                editor.faction_draft = None;
                editor.mode = if editor.npc_draft.is_some() {
                    EditorMode::Npc
                } else if editor.location_draft.is_some() {
                    EditorMode::Location
                } else {
                    EditorMode::None
                };
                ok_response("faction draft discarded.".to_string(), Some(CommandClientEvent::ClearDrafts))
            }
        }
        EditorMode::None => ok_response("no active draft to cancel.".to_string(), None),
    };
    Ok(Some(response))
}

async fn reroll_current_npc(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    use crate::commands::{npc_summary_text, npc_event_from_draft};

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.npc_draft.clone()
    };
    let Some(mut draft) = draft else {
        return Ok(Some(ok_response("no active npc draft.".to_string(), None)));
    };

    let ai = AiGenerationService;
    let seed = ai.generate_npc_seed(draft.seed_prompt.clone(), &state.workspace_root).await?;
    draft.name = seed.name.trim().to_string();
    draft.race = seed.race.trim().to_string();
    draft.occupation = normalize_unknown_text(&seed.occupation);
    draft.sex = normalize_sex(&seed.sex)?;
    draft.age = normalize_unknown_text(&seed.age);
    draft.height = normalize_unknown_text(&seed.height);
    draft.weight_lbs = normalize_unknown_text(&seed.weight_lbs);
    draft.background = normalize_unknown_text(&seed.background);
    draft.want_need = normalize_unknown_text(&seed.want_need);
    draft.secret_obstacle = normalize_unknown_text(&seed.secret_obstacle);
    draft.carrying = normalize_unknown_list(seed.carrying);

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Npc;
        editor.location_draft = None;
        editor.npc_draft = Some(draft.clone());
    }

    Ok(Some(ok_response(npc_summary_text(&draft), Some(npc_event_from_draft(&draft)))))
}

async fn reroll_current_location(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    use crate::commands::{location_summary_text, location_event_from_draft};

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.location_draft.clone()
    };
    let Some(mut draft) = draft else {
        return Ok(Some(ok_response("no active location draft.".to_string(), None)));
    };

    let ai = AiGenerationService;
    let seed = ai.generate_location_seed(draft.seed_prompt.clone(), &state.workspace_root).await?;
    draft.name = seed.name;
    draft.kind_type = seed.kind_type;
    draft.kind_custom = seed.kind_custom;
    draft.visual_description = seed.visual_description;
    draft.history_background = seed.history_background;
    draft.exports = seed.exports;
    draft.tone = seed.tone;
    draft.authority = seed.authority;
    draft.danger_level = seed.danger_level;
    draft.current_tension = seed.current_tension;

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Location;
        editor.npc_draft = None;
        editor.location_draft = Some(draft.clone());
    }

    Ok(Some(ok_response(location_summary_text(&draft), Some(location_event_from_draft(&draft)))))
}

async fn reroll_current_faction(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    use crate::commands::{faction_summary_text, faction_event_from_draft};

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.faction_draft.clone()
    };
    let Some(mut draft) = draft else {
        return Ok(Some(ok_response("no active faction draft.".to_string(), None)));
    };

    let ai = AiGenerationService;
    let seed = ai.generate_faction_seed(draft.seed_prompt.clone(), &state.workspace_root).await?;
    draft.name = seed.name;
    draft.kind_type = seed.kind_type;
    draft.kind_custom = seed.kind_custom;
    draft.public_description = seed.public_description;
    draft.true_agenda = seed.true_agenda;
    draft.methods = seed.methods;
    draft.leadership = seed.leadership;
    draft.headquarters = seed.headquarters;
    draft.sphere_of_influence = seed.sphere_of_influence;
    draft.resources_assets = seed.resources_assets;
    draft.allies = seed.allies;
    draft.rivals_enemies = seed.rivals_enemies;
    draft.reputation = seed.reputation;
    draft.current_tension = seed.current_tension;
    draft.goals_short_term = seed.goals_short_term;
    draft.goals_long_term = seed.goals_long_term;
    draft.symbol_description = seed.symbol_description;

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Faction;
        editor.npc_draft = None;
        editor.location_draft = None;
        editor.faction_draft = Some(draft.clone());
    }

    Ok(Some(ok_response(faction_summary_text(&draft), Some(faction_event_from_draft(&draft)))))
}

async fn npc_save(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    use crate::utils::SaveNpcDraftInput;

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.npc_draft.clone()
    }.ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

    let result = save_npc_draft(
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
        state.clone(),
    ).await?;

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::None;
        editor.npc_draft = None;
        editor.location_draft = None;
        editor.faction_draft = None;
    }

    let output = [
        "## NPC saved".to_string(),
        format!("id: {}", result.id),
        format!("slug: {}", result.slug),
        format!("vault: {}", path_for_display(&result.vault_path)),
        format!("updated: {}", result.updated_at),
    ].join("\n");

    Ok(Some(ok_response(output, Some(CommandClientEvent::ClearDrafts))))
}

async fn location_save(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    use crate::utils::SaveLocationDraftInput;

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.location_draft.clone()
    }.ok_or_else(|| "no active location draft. run create location or load <name>.".to_string())?;

    let result = save_location_draft(
        SaveLocationDraftInput {
            id: draft.id.clone(),
            name: draft.name.clone(),
            slug: draft.slug.clone(),
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
        state.clone(),
    ).await?;

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::None;
        editor.npc_draft = None;
        editor.location_draft = None;
        editor.faction_draft = None;
    }

    let output = [
        "## Location saved".to_string(),
        format!("id: {}", result.id),
        format!("slug: {}", result.slug),
        format!("vault: {}", path_for_display(&result.vault_path)),
        format!("updated: {}", result.updated_at),
    ].join("\n");

    Ok(Some(ok_response(output, Some(CommandClientEvent::ClearDrafts))))
}

async fn faction_save(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    use crate::utils::SaveFactionDraftInput;

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.faction_draft.clone()
    }.ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;

    let result = save_faction_draft(
        SaveFactionDraftInput {
            id: draft.id.clone(),
            slug: draft.slug.clone(),
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
        state.clone(),
    ).await?;

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::None;
        editor.npc_draft = None;
        editor.location_draft = None;
        editor.faction_draft = None;
    }

    let output = [
        "## Faction saved".to_string(),
        format!("id: {}", result.id),
        format!("slug: {}", result.slug),
        format!("vault: {}", path_for_display(&result.vault_path)),
        format!("updated: {}", result.updated_at),
    ].join("\n");

    Ok(Some(ok_response(output, Some(CommandClientEvent::ClearDrafts))))
}

pub fn normalize_unknown_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() { "Unknown".to_string() } else { trimmed.to_string() }
}

pub fn normalize_unknown_list(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values.into_iter().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).collect();
    if cleaned.is_empty() { vec!["Unknown".to_string()] } else { cleaned }
}

pub fn normalize_sex(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "male" || normalized == "female" { Ok(normalized) } else { Err("sex must be one of: male, female".to_string()) }
}

pub fn path_for_display(path: &str) -> String {
    if std::path::MAIN_SEPARATOR == '\\' { path.replace('/', "\\") } else { path.replace('\\', "/") }
}

async fn save_npc_draft(input: crate::utils::SaveNpcDraftInput, state: tauri::State<'_, AppState>) -> Result<crate::utils::SaveNpcDraftResult, String> {
    crate::utils::save_npc_draft_impl(input, state).await
}

async fn save_location_draft(input: crate::utils::SaveLocationDraftInput, state: tauri::State<'_, AppState>) -> Result<crate::utils::SaveLocationDraftResult, String> {
    crate::utils::save_location_draft_impl(input, state).await
}

async fn save_faction_draft(input: crate::utils::SaveFactionDraftInput, state: tauri::State<'_, AppState>) -> Result<crate::utils::SaveFactionDraftResult, String> {
    crate::utils::save_faction_draft_impl(input, state).await
}
