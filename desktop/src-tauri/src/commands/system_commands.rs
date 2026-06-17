use crate::app_state::AppState;
use crate::commands::{
    faction_event_from_draft, faction_summary_text, DesktopHandlerInvocation,
};
use crate::entities::common::{
    command_message_response,
    entity_message_response,
    entity_response_with_event,
    merge_seed_and_reroll_prompt,
};
use crate::commands::ok_response_with_doc;
use crate::entities::EntityKind;
use crate::services::ai_generation::{AiGenerationService, SeedGeneration};
use crate::utils::{normalize_optional_prompt, normalize_sex, normalize_unknown_list, normalize_unknown_text, prepend_notice};
use dnd_core::command::render_help_overview;
use dnd_core::command_manifest::InputContext;
use dnd_core::npc::slugify;
use runebound_models::CommandResponse;
use runebound_models::dungeon_plan::roll_dungeon_content_plan;


pub async fn handle_help(invocation: DesktopHandlerInvocation<'_>) -> Result<Option<CommandResponse>, String> {
    // Resolve the current input context so the help index lists the commands
    // actually runnable here — an open entity editor takes precedence over an
    // in-progress setup wizard. Mirrors the suggestion service's context logic.
    let active_kind = {
        let editor = invocation.state.editor_session.lock().await;
        editor.active_kind()
    };
    let context = match active_kind {
        Some(kind) => InputContext::EntityEditor(kind.as_str().to_string()),
        None => {
            let onboarding_active = {
                let service = invocation.state.command_service.lock().await;
                service.session().onboarding.active
            };
            if onboarding_active {
                InputContext::ConfigEditor
            } else {
                InputContext::Default
            }
        }
    };

    let overview = render_help_overview(&context);
    Ok(Some(ok_response_with_doc(
        overview.output,
        overview.output_doc,
        None,
    )))
}

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
        None => command_message_response("no active draft to save."),
    }
}

pub async fn handle_reroll(invocation: DesktopHandlerInvocation<'_>) -> Result<Option<CommandResponse>, String> {
    let active_kind = {
        let editor = invocation.state.editor_session.lock().await;
        editor.active_kind()
    };

    let reroll_prompt = if invocation.lowered.len() > 1 {
        let raw_after_reroll = invocation.raw_input.trim_start_matches(|c: char| c.is_whitespace());
        if let Some(stripped) = raw_after_reroll.strip_prefix("reroll") {
            let after_reroll = stripped.trim_start_matches(|c: char| c.is_whitespace());
            if !after_reroll.is_empty() {
                normalize_optional_prompt(Some(after_reroll.to_string()))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    match active_kind {
        Some(EntityKind::Npc) => reroll_current_npc(invocation.state.clone(), reroll_prompt).await,
        Some(EntityKind::Location) => reroll_current_location(invocation.state.clone(), reroll_prompt).await,
        Some(EntityKind::Faction) => reroll_current_faction(invocation.state.clone(), reroll_prompt).await,
        Some(EntityKind::Item) => reroll_current_item(invocation.state.clone(), reroll_prompt).await,
        Some(EntityKind::Event) => reroll_current_event(invocation.state.clone(), reroll_prompt).await,
        Some(EntityKind::God) => reroll_current_god(invocation.state.clone(), reroll_prompt).await,
        Some(EntityKind::Dungeon) => reroll_current_dungeon(invocation.state.clone(), reroll_prompt).await,
        None => command_message_response("no active draft to reroll."),
    }
}

async fn reroll_current_dungeon(state: tauri::State<'_, AppState>, reroll_prompt: Option<String>) -> Result<Option<CommandResponse>, String> {
    use crate::commands::{dungeon_event_from_draft, dungeon_summary_text};

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.get_dungeon().cloned()
    };
    let Some(mut draft) = draft else {
        return entity_message_response("no active dungeon draft.");
    };

    // Whole-dungeon regen re-runs the two-pass generator from the stored seed +
    // the dials, rolling a fresh content plan (variety across rerolls) and
    // replacing all five beats while keeping tone/twist/topology authoritative.
    let merged_prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, reroll_prompt);
    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();

    let plan = roll_dungeon_content_plan(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0),
    );
    let SeedGeneration { seed: story, .. } = ai
        .generate_dungeon_story(
            &plan,
            merged_prompt,
            "",
            &draft.tone,
            &draft.twist,
            &draft.topology,
            None,
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;
    let SeedGeneration { seed, notice } = ai
        .structure_dungeon_story(
            &plan,
            &story,
            &draft.tone,
            &draft.twist,
            &draft.topology,
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;
    draft.slug = slugify(seed.name.trim());
    draft.name = seed.name.trim().to_string();
    draft.location = normalize_unknown_text(&seed.location);
    draft.story = story.story.clone();
    draft.premise = normalize_unknown_text(&seed.premise);
    draft.beats = seed.into_beats();

    {
        let mut editor = state.editor_session.lock().await;
        editor.set_dungeon(draft.clone());
    }

    entity_response_with_event(
        prepend_notice(notice, dungeon_summary_text(&draft)),
        dungeon_event_from_draft(&draft),
    )
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
        None => command_message_response("no active draft to cancel."),
    }
}

async fn reroll_current_event(state: tauri::State<'_, AppState>, reroll_prompt: Option<String>) -> Result<Option<CommandResponse>, String> {
    // Events regenerate as a whole, which is exactly what the domain's
    // (field-agnostic) reroll does — route through it so the logic lives once.
    let domain = state
        .domains()
        .domain(EntityKind::Event)
        .expect("event domain not registered");
    domain.reroll_field("", reroll_prompt, state.inner()).await
}

async fn reroll_current_npc(state: tauri::State<'_, AppState>, reroll_prompt: Option<String>) -> Result<Option<CommandResponse>, String> {
    use crate::commands::{npc_summary_text, npc_event_from_draft};

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.get_npc().cloned()
    };
    let Some(mut draft) = draft else {
        return entity_message_response("no active npc draft.");
    };

    let merged_prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, reroll_prompt);
    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let SeedGeneration { seed, notice } = ai
        .generate_npc_seed(
            merged_prompt,
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;
    draft.slug = slugify(&seed.name.trim());
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

    entity_response_with_event(
        prepend_notice(notice, npc_summary_text(&draft)),
        npc_event_from_draft(&draft),
    )
}

async fn reroll_current_location(state: tauri::State<'_, AppState>, reroll_prompt: Option<String>) -> Result<Option<CommandResponse>, String> {
    use crate::commands::{location_summary_text, location_event_from_draft};

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.get_location().cloned()
    };
    let Some(mut draft) = draft else {
        return entity_message_response("no active location draft.");
    };

    let merged_prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, reroll_prompt);
    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let SeedGeneration { seed, notice } = ai
        .generate_location_seed(
            merged_prompt,
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;
    draft.slug = slugify(&seed.name);
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

    entity_response_with_event(
        prepend_notice(notice, location_summary_text(&draft)),
        location_event_from_draft(&draft),
    )
}

async fn reroll_current_faction(state: tauri::State<'_, AppState>, reroll_prompt: Option<String>) -> Result<Option<CommandResponse>, String> {
    let draft = {
        let editor = state.editor_session.lock().await;
        editor.get_faction().cloned()
    };
    let Some(mut draft) = draft else {
        return entity_message_response("no active faction draft.");
    };

    let merged_prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, reroll_prompt);
    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let SeedGeneration { seed, notice } = ai
        .generate_faction_seed(
            merged_prompt,
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;
    draft.slug = slugify(&seed.name);
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

    entity_response_with_event(
        prepend_notice(notice, faction_summary_text(&draft)),
        faction_event_from_draft(&draft),
    )
}

async fn reroll_current_god(state: tauri::State<'_, AppState>, reroll_prompt: Option<String>) -> Result<Option<CommandResponse>, String> {
    use crate::commands::{god_event_from_draft, god_summary_text};

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.get_god().cloned()
    };
    let Some(mut draft) = draft else {
        return entity_message_response("no active god draft.");
    };

    let merged_prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, reroll_prompt);
    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let SeedGeneration { seed, notice } = ai
        .generate_god_seed(
            merged_prompt,
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;
    draft.slug = slugify(&seed.name);
    draft.name = seed.name;
    draft.epithet = seed.epithet;
    draft.rank = seed.rank;
    draft.rank_custom = seed.rank_custom;
    draft.alignment = seed.alignment;
    draft.domains = seed.domains;
    draft.symbol = seed.symbol;
    draft.appearance = seed.appearance;
    draft.dogma = seed.dogma;
    draft.realm = seed.realm;
    draft.worshippers = seed.worshippers;
    draft.clergy = seed.clergy;
    draft.allies = seed.allies;
    draft.rivals = seed.rivals;

    {
        let mut editor = state.editor_session.lock().await;
        editor.set_god(draft.clone());
        editor.clear_kind(EntityKind::Npc);
        editor.clear_kind(EntityKind::Location);
    }

    entity_response_with_event(
        prepend_notice(notice, god_summary_text(&draft)),
        god_event_from_draft(&draft),
    )
}

async fn reroll_current_item(state: tauri::State<'_, AppState>, reroll_prompt: Option<String>) -> Result<Option<CommandResponse>, String> {
    use crate::commands::{item_event_from_draft, item_summary_text};

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.get_item().cloned()
    };
    let Some(mut draft) = draft else {
        return entity_message_response("no active item draft.");
    };

    let merged_prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, reroll_prompt);
    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let SeedGeneration { seed, notice } = ai
        .generate_item_seed(
            merged_prompt,
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;
    draft.slug = slugify(&seed.name);
    draft.name = seed.name;
    draft.category = seed.category;
    draft.rarity = seed.rarity;
    draft.attunement = seed.attunement;
    draft.materials = seed.materials;
    draft.appearance = seed.appearance;
    draft.abilities = seed.abilities;
    draft.drawbacks = seed.drawbacks;
    draft.history = seed.history;
    draft.value = seed.value;
    draft.location = seed.location;

    {
        let mut editor = state.editor_session.lock().await;
        editor.set_item(draft.clone());
    }

    entity_response_with_event(
        prepend_notice(notice, item_summary_text(&draft)),
        item_event_from_draft(&draft),
    )
}
