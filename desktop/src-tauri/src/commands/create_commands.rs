use crate::app_state::AppState;
use crate::commands::{
    DesktopHandlerInvocation, faction_event_from_draft, faction_summary_text,
    item_event_from_draft, item_summary_text, location_event_from_draft, location_summary_text,
    npc_event_from_draft, npc_summary_text,
};
use crate::entities::common::{
    command_message_response,
    command_response_with_event,
    CommandResult,
};
use crate::entities::EntityKind;
use crate::services::ai_generation::AiGenerationService;
use crate::utils::{
    normalize_optional_prompt,
    normalize_sex,
    normalize_unknown_list,
    normalize_unknown_text,
};
use dnd_core::npc::UNKNOWN_LOCATION;

use crate::app_state::{FactionDraftSession, ItemDraftSession, LocationDraftSession, NpcDraftSession};

pub async fn handle_create(
    invocation: DesktopHandlerInvocation<'_>,
) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "create help" {
        return command_message_response([
            "## Create commands",
            "create npc",
            "create npc <prompt text>",
            "create location",
            "create location <prompt text>",
            "create faction",
            "create faction <prompt text>",
            "create item",
            "create item <prompt text>",
        ].join("\n"));
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

    if lowered == "create item" || lowered.starts_with("create item ") {
        return create_item(trimmed, invocation.state.clone()).await;
    }

    command_message_response("unknown create command. use `create help`")
}

async fn create_npc(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> CommandResult {
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

    command_response_with_event(npc_summary_text(&draft), npc_event_from_draft(&draft))
}

async fn create_location(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> CommandResult {
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

    command_response_with_event(
        location_summary_text(&draft),
        location_event_from_draft(&draft),
    )
}

async fn create_faction(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> CommandResult {
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

    command_response_with_event(
        faction_summary_text(&draft),
        faction_event_from_draft(&draft),
    )
}

async fn create_item(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> CommandResult {
    use dnd_core::npc::slugify;

    let prompt = if trimmed.len() > 11 {
        let value = trimmed[11..].trim();
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
        .generate_item_seed(
            prompt.clone(),
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;

    let slug = slugify(&seed.name);
    let draft = ItemDraftSession {
        id: make_entity_id("item"),
        seed_prompt: prompt,
        name: seed.name,
        slug,
        vault_path: String::new(),
        category: seed.category,
        rarity: seed.rarity,
        attunement: seed.attunement,
        materials: seed.materials,
        appearance: seed.appearance,
        abilities: seed.abilities,
        drawbacks: seed.drawbacks,
        history: seed.history,
        value_gp: seed.value_gp,
        current_owner: seed.current_owner,
        location: seed.location,
    };

    {
        let mut editor = state.editor_session.lock().await;
        editor.set_item(draft.clone());
        editor.clear_kind(EntityKind::Npc);
        editor.clear_kind(EntityKind::Location);
        editor.clear_kind(EntityKind::Faction);
    }

    command_response_with_event(item_summary_text(&draft), item_event_from_draft(&draft))
}

fn make_entity_id(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let micros = timestamp.as_micros() as u64;
    format!("{}_{:x}{:x}", prefix, micros >> 16, micros & 0xFFFF)
}
