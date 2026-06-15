use crate::app_state::AppState;
use crate::commands::{
    ok_response, DesktopHandlerInvocation, faction_event_from_draft, faction_summary_text,
    location_event_from_draft, location_summary_text, npc_event_from_draft, npc_summary_text,
};
use crate::entities::EntityKind;
use crate::services::ai_generation::AiGenerationService;
use dnd_core::npc::UNKNOWN_LOCATION;
use runebound_models::CommandResponse;

use crate::app_state::{FactionDraftSession, LocationDraftSession, NpcDraftSession};

pub async fn handle_create(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "create help" {
        return Ok(Some(ok_response(
            [
                "## Create commands",
                "create npc",
                "create npc <prompt text>",
                "create location",
                "create location <prompt text>",
                "create faction",
                "create faction <prompt text>",
            ]
            .join("\n"),
            None,
        )));
    }

    if lowered == "create npc" || lowered.starts_with("create npc ") {
        return create_npc(trimmed, invocation.state.clone()).await;
    }

    if lowered == "create location" || lowered.starts_with("create location ") {
        return create_location(trimmed, invocation.state.clone()).await;
    }

    if lowered == "create faction" || lowered.starts_with("create faction ") {
        return create_faction(trimmed, invocation.state.clone()).await;
    }

    Ok(Some(ok_response(
        "unknown create command. use `create help`".to_string(),
        None,
    )))
}

async fn create_npc(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    let prompt = if trimmed.len() > 10 {
        let value = trimmed[10..].trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    } else {
        None
    };

    let prompt = normalize_optional_prompt(prompt);

    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let seed = ai
        .generate_npc_seed(
            prompt.clone(),
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;

    let draft = NpcDraftSession {
        id: make_entity_id("npc"),
        seed_prompt: prompt,
        name: seed.name.trim().to_string(),
        race: seed.race.trim().to_string(),
        occupation: normalize_unknown_text(&seed.occupation),
        sex: normalize_sex(&seed.sex)?,
        age: normalize_unknown_text(&seed.age),
        height: normalize_unknown_text(&seed.height),
        weight_lbs: normalize_unknown_text(&seed.weight_lbs),
        background: normalize_unknown_text(&seed.background),
        want_need: normalize_unknown_text(&seed.want_need),
        secret_obstacle: normalize_unknown_text(&seed.secret_obstacle),
        carrying: normalize_unknown_list(seed.carrying),
        location: UNKNOWN_LOCATION.to_string(),
    };

    {
        let mut editor = state.editor_session.lock().await;
        editor.set_npc(draft.clone());
        editor.clear_kind(EntityKind::Location);
    }

    Ok(Some(ok_response(
        npc_summary_text(&draft),
        Some(npc_event_from_draft(&draft)),
    )))
}

async fn create_location(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    use dnd_core::npc::slugify;

    let prompt = if trimmed.len() > 15 {
        let value = trimmed[15..].trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    } else {
        None
    };

    let prompt = normalize_optional_prompt(prompt);

    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let seed = ai
        .generate_location_seed(
            prompt.clone(),
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;

    let draft = LocationDraftSession {
        id: make_entity_id("loc"),
        seed_prompt: prompt,
        slug: slugify(&seed.name),
        name: seed.name,
        vault_path: String::new(),
        kind_type: seed.kind_type,
        kind_custom: seed.kind_custom,
        visual_description: seed.visual_description,
        history_background: seed.history_background,
        exports: seed.exports,
        tone: seed.tone,
        authority: seed.authority,
        danger_level: seed.danger_level,
        current_tension: seed.current_tension,
    };

    {
        let mut editor = state.editor_session.lock().await;
        editor.set_location(draft.clone());
        editor.clear_kind(EntityKind::Npc);
    }

    Ok(Some(ok_response(
        location_summary_text(&draft),
        Some(location_event_from_draft(&draft)),
    )))
}

async fn create_faction(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    use dnd_core::npc::slugify;

    let prompt = if trimmed.len() > 14 {
        let value = trimmed[14..].trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    } else {
        None
    };

    let prompt = normalize_optional_prompt(prompt);

    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let seed = ai
        .generate_faction_seed(
            prompt.clone(),
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;

    let draft = FactionDraftSession {
        id: make_entity_id("fac"),
        seed_prompt: prompt,
        slug: slugify(&seed.name),
        name: seed.name,
        vault_path: String::new(),
        kind_type: seed.kind_type,
        kind_custom: seed.kind_custom,
        public_description: seed.public_description,
        true_agenda: seed.true_agenda,
        methods: seed.methods,
        leadership: seed.leadership,
        headquarters: seed.headquarters,
        sphere_of_influence: seed.sphere_of_influence,
        resources_assets: seed.resources_assets,
        allies: seed.allies,
        rivals_enemies: seed.rivals_enemies,
        reputation: seed.reputation,
        current_tension: seed.current_tension,
        goals_short_term: seed.goals_short_term,
        goals_long_term: seed.goals_long_term,
        symbol_description: seed.symbol_description,
    };

    {
        let mut editor = state.editor_session.lock().await;
        editor.set_faction(draft.clone());
        editor.clear_kind(EntityKind::Npc);
        editor.clear_kind(EntityKind::Location);
    }

    Ok(Some(ok_response(
        faction_summary_text(&draft),
        Some(faction_event_from_draft(&draft)),
    )))
}

fn normalize_optional_prompt(prompt: Option<String>) -> Option<String> {
    prompt.map(|p| {
        let trimmed = p.trim();
        if trimmed.is_empty() {
            String::new()
        } else {
            trimmed.to_string()
        }
    })
}

fn normalize_unknown_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_unknown_list(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if cleaned.is_empty() {
        vec!["Unknown".to_string()]
    } else {
        cleaned
    }
}

fn normalize_sex(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "male" || normalized == "female" {
        Ok(normalized)
    } else {
        Err("sex must be one of: male, female".to_string())
    }
}

fn make_entity_id(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let micros = timestamp.as_micros() as u64;
    format!("{}_{:x}{:x}", prefix, micros >> 16, micros & 0xFFFF)
}
