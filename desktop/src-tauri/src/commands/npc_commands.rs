use crate::app_state::{AppState, EditorMode};
use crate::commands::{ok_response, DesktopHandlerInvocation};
use crate::entities::{
    canonical_field_name,
    format_valid_field_list,
    EntityKind,
    FieldAccess,
};
use crate::services::entity_admin::{EntityAdminService, EnsureLocationInput};
use crate::services::entity_persistence::{EntityPersistenceService, SaveNpcDraftInput};
use crate::services::entity_reroll::{
    EntityRerollService, NpcRerollContext, RerollNpcFieldInput,
};
use crate::utils::{
    normalize_optional_prompt, normalize_sex, parse_carrying_csv, path_for_display,
};
use crate::app_state::NpcDraftSession;
use dnd_core::command::CommandClientEvent;
use runebound_models::CommandResponse;

pub async fn handle_npc(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "npc help" {
        let has_draft = {
            let editor = invocation.state.editor_session.lock().await;
            editor.npc_draft.is_some()
        };
        if !has_draft {
            return Ok(Some(ok_response(
                "no active npc draft. run create npc or load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            [
                "## NPC editor commands",
                "npc show",
                "npc rename <name>",
                "npc set <field> <value>",
                "npc travel to <location>",
                "npc reroll <field> [prompt]",
                "reroll",
                "npc save",
                "npc cancel",
            ]
            .join("\n"),
            None,
        )));
    }

    if lowered == "npc show" {
        let draft = {
            let editor = invocation.state.editor_session.lock().await;
            editor.npc_draft.clone()
        };
        let Some(draft) = draft else {
            return Ok(Some(ok_response(
                "no active npc draft. run create npc or load <name>.".to_string(),
                None,
            )));
        };
        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered == "npc cancel" {
        let had_draft = {
            let mut editor = invocation.state.editor_session.lock().await;
            let had = editor.npc_draft.is_some();
            if had {
                editor.npc_draft = None;
                editor.mode = if editor.location_draft.is_some() {
                    EditorMode::Location
                } else if editor.faction_draft.is_some() {
                    EditorMode::Faction
                } else {
                    EditorMode::None
                };
            }
            had
        };
        if !had_draft {
            return Ok(Some(ok_response(
                "no active npc draft. run create npc or load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            "npc draft discarded.".to_string(),
            Some(CommandClientEvent::ClearDrafts),
        )));
    }

    if lowered.starts_with("npc rename ") {
        return npc_rename(trimmed, invocation.state.clone()).await;
    }

    if lowered.starts_with("npc set ") {
        return npc_set(trimmed, invocation.state.clone()).await;
    }

    if lowered.starts_with("npc travel ") {
        return npc_travel(trimmed, invocation.state.clone()).await;
    }

    if lowered == "npc save" {
        return npc_save(invocation.state.clone()).await;
    }

    if lowered == "npc reroll" || lowered.starts_with("npc reroll ") {
        return npc_reroll(trimmed, invocation.state.clone()).await;
    }

    Ok(Some(ok_response(
        "unknown npc command. use `npc help`".to_string(),
        None,
    )))
}

pub async fn npc_rename(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    let name = trimmed[10..].trim();
    if name.is_empty() {
        return Ok(Some(ok_response("npc name cannot be empty.".to_string(), None)));
    }
    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.npc_draft.clone()
    }.ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;
    draft.name = name.to_string();
    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Npc;
        editor.npc_draft = Some(draft.clone());
        editor.location_draft = None;
    }
    Ok(Some(ok_response(npc_summary_text(&draft), Some(npc_event_from_draft(&draft)))))
}

pub async fn npc_set(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    let mut parts = trimmed.splitn(4, char::is_whitespace);
    let _ = parts.next();
    let _ = parts.next();
    let field = parts.next().unwrap_or_default();
    let value = parts.next().unwrap_or_default().trim();
    if value.is_empty() {
        return Ok(Some(ok_response("npc set value cannot be empty.".to_string(), None)));
    }

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.npc_draft.clone()
    }.ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

    let Some(canonical) =
        canonical_field_name(EntityKind::Npc, field, FieldAccess::Set)
    else {
        let valid_fields = format_valid_field_list(EntityKind::Npc, FieldAccess::Set);
        return Ok(Some(ok_response(
            format!("unknown npc field: {}. valid fields: {}", field, valid_fields),
            None,
        )));
    };

    match canonical {
        "name" => draft.name = value.to_string(),
        "race" => draft.race = value.to_string(),
        "occupation" => draft.occupation = value.to_string(),
        "sex" => draft.sex = normalize_sex(value)?,
        "age" => draft.age = value.to_string(),
        "height" => draft.height = value.to_string(),
        "weight_lbs" => draft.weight_lbs = value.to_string(),
        "background" => draft.background = value.to_string(),
        "want_need" => draft.want_need = value.to_string(),
        "secret_obstacle" => draft.secret_obstacle = value.to_string(),
        "carrying" => draft.carrying = parse_carrying_csv(value),
        _ => {}
    }

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Npc;
        editor.npc_draft = Some(draft.clone());
        editor.location_draft = None;
    }

    Ok(Some(ok_response(npc_summary_text(&draft), Some(npc_event_from_draft(&draft)))))
}

pub async fn npc_travel(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    if !trimmed.to_ascii_lowercase().starts_with("npc travel to ") {
        return Ok(Some(ok_response("usage: npc travel to <location>".to_string(), None)));
    }
    let location_name = trimmed[14..].trim();
    if location_name.is_empty() {
        return Ok(Some(ok_response("location cannot be empty.".to_string(), None)));
    }

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.npc_draft.clone()
    }.ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

    let admin = EntityAdminService;
    let result = admin
        .ensure_location_exists(
            EnsureLocationInput {
                name: location_name.to_string(),
            },
            state.inner(),
        )
        .await?;
    draft.location = if result.name.trim().is_empty() { location_name.to_string() } else { result.name };

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Npc;
        editor.npc_draft = Some(draft.clone());
        editor.location_draft = None;
    }

    Ok(Some(ok_response(npc_summary_text(&draft), Some(npc_event_from_draft(&draft)))))
}

pub async fn npc_save(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    let draft = {
        let editor = state.editor_session.lock().await;
        editor.npc_draft.clone()
    }.ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

    let persistence = EntityPersistenceService;
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
            state.inner(),
        )
        .await?;

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

pub async fn npc_reroll(
    trimmed: &str,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    if trimmed.eq_ignore_ascii_case("npc reroll") {
        return Ok(Some(ok_response("usage: npc reroll <field> [prompt]".to_string(), None)));
    }
    if trimmed.len() <= 11 {
        return Ok(Some(ok_response("usage: npc reroll <field> [prompt]".to_string(), None)));
    }
    let args = trimmed[11..].trim();
    if args.is_empty() {
        return Ok(Some(ok_response("usage: npc reroll <field> [prompt]".to_string(), None)));
    }
    let mut split = args.splitn(2, char::is_whitespace);
    let field = split.next().unwrap_or_default().trim().to_string();
    let prompt = normalize_optional_prompt(split.next().map(|value| value.to_string()));

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.npc_draft.clone()
    }.ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

    let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

    let reroll_service = EntityRerollService;
    let workspace_root = state.workspace_root.clone();
    let database = state.database();
    let generation_repo = state.generation_repo();
    let rerolled = reroll_service
        .reroll_npc_field(
            RerollNpcFieldInput {
                field,
                prompt,
                npc: NpcRerollContext {
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
            },
            &workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;

    match rerolled.field.as_str() {
        "name" => { if let Some(value) = rerolled.value { draft.name = value; } }
        "race" => { if let Some(value) = rerolled.value { draft.race = value; } }
        "occupation" => { if let Some(value) = rerolled.value { draft.occupation = value; } }
        "sex" => { if let Some(value) = rerolled.value { draft.sex = normalize_sex(&value)?; } }
        "age" => { if let Some(value) = rerolled.value { draft.age = value; } }
        "height" => { if let Some(value) = rerolled.value { draft.height = value; } }
        "weight_lbs" => { if let Some(value) = rerolled.value { draft.weight_lbs = value; } }
        "background" => { if let Some(value) = rerolled.value { draft.background = value; } }
        "want_need" => { if let Some(value) = rerolled.value { draft.want_need = value; } }
        "secret_obstacle" => { if let Some(value) = rerolled.value { draft.secret_obstacle = value; } }
        "carrying" => { if let Some(carrying) = rerolled.carrying { draft.carrying = carrying; } }
        _ => {}
    }

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Npc;
        editor.npc_draft = Some(draft.clone());
        editor.location_draft = None;
    }

    Ok(Some(ok_response(npc_summary_text(&draft), Some(npc_event_from_draft(&draft)))))
}

fn merge_seed_and_reroll_prompt(seed_prompt: &Option<String>, reroll_prompt: Option<String>) -> Option<String> {
    let seed_prompt = seed_prompt.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty());
    let reroll_prompt = reroll_prompt.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty());

    match (seed_prompt, reroll_prompt) {
        (Some(seed), Some(reroll)) => Some(format!("Seed context from original create command:\n{}\n\nReroll request:\n{}", seed, reroll)),
        (Some(seed), None) => Some(seed.to_string()),
        (None, Some(reroll)) => Some(reroll.to_string()),
        (None, None) => None,
    }
}

pub fn npc_summary_text(draft: &NpcDraftSession) -> String {
    format!(
        "## NPC Draft\nname: {}\nrace: {}\noccupation: {}\nsex: {}\nage: {}\nheight: {}\nweight: {}\nbackground: {}\nwant: {}\nsecret: {}\ncarrying: {}\nlocation: {}",
        draft.name,
        draft.race,
        draft.occupation,
        draft.sex,
        draft.age,
        draft.height,
        draft.weight_lbs,
        draft.background,
        draft.want_need,
        draft.secret_obstacle,
        draft.carrying.join(", "),
        draft.location,
    )
}

pub fn npc_event_from_draft(draft: &NpcDraftSession) -> CommandClientEvent {
    use runebound_models::drafts::npc_entity_card;
    use dnd_core::npc::normalize_unknown_text as core_normalize_unknown;
    use dnd_core::npc::normalize_unknown_list as core_normalize_list;

    let normalized_draft = NpcDraftSession {
        id: draft.id.clone(),
        name: draft.name.clone(),
        race: core_normalize_unknown(&draft.race),
        occupation: core_normalize_unknown(&draft.occupation),
        sex: match draft.sex.to_lowercase().as_str() {
            "male" => "Male".to_string(),
            "female" => "Female".to_string(),
            _ => draft.sex.clone(),
        },
        age: core_normalize_unknown(&draft.age),
        height: core_normalize_unknown(&draft.height),
        weight_lbs: core_normalize_unknown(&draft.weight_lbs),
        background: core_normalize_unknown(&draft.background),
        want_need: core_normalize_unknown(&draft.want_need),
        secret_obstacle: core_normalize_unknown(&draft.secret_obstacle),
        carrying: core_normalize_list(draft.carrying.clone()),
        location: core_normalize_unknown(&draft.location),
        seed_prompt: draft.seed_prompt.clone(),
    };
    let entity_card_doc = npc_entity_card(&normalized_draft);
    CommandClientEvent::LoadNpcDraftWithCard {
        draft: normalized_draft,
        entity_card: entity_card_doc,
    }
}
