use crate::app_state::AppState;
use crate::commands::{
    faction_event_from_draft, faction_summary_text, ok_response, DesktopHandlerInvocation,
};
use crate::entities::EntityKind;
use crate::services::ai_generation::AiGenerationService;
use runebound_models::CommandResponse;


pub async fn handle_save(invocation: DesktopHandlerInvocation<'_>) -> Result<Option<CommandResponse>, String> {
    let active_kind = {
        let editor = invocation.state.editor_session.lock().await;
        editor.active_kind()
    };

    match active_kind {
        Some(kind) => {
            let domain = invocation
                .state
                .domains()
                .domain(kind)
                .expect("domain not registered");
            domain.save(invocation.state.inner()).await
        }
        None => Ok(Some(ok_response("no active draft to save.".to_string(), None))),
    }
}

pub async fn handle_reroll(invocation: DesktopHandlerInvocation<'_>) -> Result<Option<CommandResponse>, String> {
    let active_kind = {
        let editor = invocation.state.editor_session.lock().await;
        editor.active_kind()
    };

    match active_kind {
        Some(EntityKind::Npc) => reroll_current_npc(invocation.state.clone()).await,
        Some(EntityKind::Location) => reroll_current_location(invocation.state.clone()).await,
        Some(EntityKind::Faction) => reroll_current_faction(invocation.state.clone()).await,
        None => Ok(Some(ok_response("no active draft to reroll.".to_string(), None))),
    }
}

pub async fn handle_cancel(invocation: DesktopHandlerInvocation<'_>) -> Result<Option<CommandResponse>, String> {
    let active_kind = {
        let editor = invocation.state.editor_session.lock().await;
        editor.active_kind()
    };

    match active_kind {
        Some(kind) => {
            let domain = invocation
                .state
                .domains()
                .domain(kind)
                .expect("domain not registered");
            domain.cancel(invocation.state.inner()).await
        }
        None => Ok(Some(ok_response(
            "no active draft to cancel.".to_string(),
            None,
        ))),
    }
}

async fn reroll_current_npc(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    use crate::commands::{npc_summary_text, npc_event_from_draft};

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.get_npc().cloned()
    };
    let Some(mut draft) = draft else {
        return Ok(Some(ok_response("no active npc draft.".to_string(), None)));
    };

    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let seed = ai
        .generate_npc_seed(
            draft.seed_prompt.clone(),
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;
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
        editor.set_npc(draft.clone());
        editor.clear_kind(EntityKind::Location);
    }

    Ok(Some(ok_response(npc_summary_text(&draft), Some(npc_event_from_draft(&draft)))))
}

async fn reroll_current_location(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    use crate::commands::{location_summary_text, location_event_from_draft};

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.get_location().cloned()
    };
    let Some(mut draft) = draft else {
        return Ok(Some(ok_response("no active location draft.".to_string(), None)));
    };

    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let seed = ai
        .generate_location_seed(
            draft.seed_prompt.clone(),
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;
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
        editor.set_location(draft.clone());
        editor.clear_kind(EntityKind::Npc);
    }

    Ok(Some(ok_response(location_summary_text(&draft), Some(location_event_from_draft(&draft)))))
}

async fn reroll_current_faction(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    let draft = {
        let editor = state.editor_session.lock().await;
        editor.get_faction().cloned()
    };
    let Some(mut draft) = draft else {
        return Ok(Some(ok_response("no active faction draft.".to_string(), None)));
    };

    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let seed = ai
        .generate_faction_seed(
            draft.seed_prompt.clone(),
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;
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
        editor.set_faction(draft.clone());
        editor.clear_kind(EntityKind::Npc);
        editor.clear_kind(EntityKind::Location);
    }

    Ok(Some(ok_response(faction_summary_text(&draft), Some(faction_event_from_draft(&draft)))))
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
