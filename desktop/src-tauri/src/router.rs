use dnd_core::command::{CommandClientEvent, CommandResponse, OutputSegment, OutputSegmentKind};
use tauri::State;

use crate::app_state::{AppState, EditorMode, LocationDraftSession, NpcDraftSession};

use super::*;

pub(crate) async fn run_desktop_routed_command(
    input: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "exit" {
        return Ok(Some(ok_response(
            "exiting".to_string(),
            Some(CommandClientEvent::ExitRequested),
        )));
    }

    if lowered == "clear" {
        return Ok(Some(ok_response(
            String::new(),
            Some(CommandClientEvent::ClearTerminal {
                clear_history: false,
            }),
        )));
    }

    if lowered == "clear --history" || lowered == "history clear" {
        let mut service = state.command_service.lock().await;
        service.session_mut().clear_history();
        return Ok(Some(ok_response(
            String::new(),
            Some(CommandClientEvent::ClearTerminal {
                clear_history: true,
            }),
        )));
    }

    if lowered == "history" || lowered.starts_with("history ") {
        let mut tokens = trimmed.split_whitespace();
        let _ = tokens.next();
        let value = tokens.next();
        if tokens.next().is_some() {
            return Ok(Some(ok_response(
                "usage: history [limit|clear]".to_string(),
                None,
            )));
        }

        let limit = match value {
            None => 20,
            Some(raw) => match raw.parse::<usize>() {
                Ok(parsed) if parsed > 0 => parsed,
                _ => {
                    return Ok(Some(ok_response(
                        "usage: history [limit|clear]".to_string(),
                        None,
                    )));
                }
            },
        };

        let history = {
            let service = state.command_service.lock().await;
            service.session().command_history.clone()
        };
        return Ok(Some(ok_response(render_history_output(&history, limit), None)));
    }

    if lowered == "create help" {
        return Ok(Some(ok_response(
            ["## Create commands", "create npc", "create npc <prompt text>"].join("\n"),
            None,
        )));
    }

    if lowered == "create npc" || lowered.starts_with("create npc ") {
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

        let seed = generate_npc_seed(GenerateNpcSeedInput { prompt }, state.clone()).await?;
        let draft = NpcDraftSession {
            id: make_entity_id("npc"),
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
            editor.mode = EditorMode::Npc;
            editor.location_draft = None;
            editor.npc_draft = Some(draft.clone());
        }

        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered == "npc help" {
        let has_draft = {
            let editor = state.editor_session.lock().await;
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

    if lowered == "location help" {
        let has_draft = {
            let editor = state.editor_session.lock().await;
            editor.location_draft.is_some()
        };
        if !has_draft {
            return Ok(Some(ok_response(
                "no active location draft. run load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            [
                "## Location editor commands",
                "location show",
                "location rename <name>",
                "location save",
                "location cancel",
            ]
            .join("\n"),
            None,
        )));
    }

    if lowered == "npc show" {
        let draft = {
            let editor = state.editor_session.lock().await;
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

    if lowered == "location show" {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.location_draft.clone()
        };
        let Some(draft) = draft else {
            return Ok(Some(ok_response(
                "no active location draft. run load <name>.".to_string(),
                None,
            )));
        };
        return Ok(Some(ok_response(
            location_summary_text(&draft),
            Some(location_event_from_draft(&draft)),
        )));
    }

    if lowered == "npc cancel" {
        let had_draft = {
            let mut editor = state.editor_session.lock().await;
            let had = editor.npc_draft.is_some();
            if had {
                editor.npc_draft = None;
                editor.mode = if editor.location_draft.is_some() {
                    EditorMode::Location
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

    if lowered == "location cancel" {
        let had_draft = {
            let mut editor = state.editor_session.lock().await;
            let had = editor.location_draft.is_some();
            if had {
                editor.location_draft = None;
                editor.mode = if editor.npc_draft.is_some() {
                    EditorMode::Npc
                } else {
                    EditorMode::None
                };
            }
            had
        };
        if !had_draft {
            return Ok(Some(ok_response(
                "no active location draft. run load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            "location draft discarded.".to_string(),
            Some(CommandClientEvent::ClearDrafts),
        )));
    }

    if lowered == "cancel" {
        let mode = {
            let editor = state.editor_session.lock().await;
            editor.mode
        };
        if mode == EditorMode::Npc {
            let mut editor = state.editor_session.lock().await;
            editor.npc_draft = None;
            editor.mode = if editor.location_draft.is_some() {
                EditorMode::Location
            } else {
                EditorMode::None
            };
            return Ok(Some(ok_response(
                "npc draft discarded.".to_string(),
                Some(CommandClientEvent::ClearDrafts),
            )));
        }
        if mode == EditorMode::Location {
            let mut editor = state.editor_session.lock().await;
            editor.location_draft = None;
            editor.mode = if editor.npc_draft.is_some() {
                EditorMode::Npc
            } else {
                EditorMode::None
            };
            return Ok(Some(ok_response(
                "location draft discarded.".to_string(),
                Some(CommandClientEvent::ClearDrafts),
            )));
        }
    }

    if lowered == "reroll" || lowered == "npc reroll" {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.clone()
        };
        let Some(mut draft) = draft else {
            return Ok(None);
        };

        let seed = generate_npc_seed(GenerateNpcSeedInput { prompt: None }, state.clone()).await?;
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

        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered.starts_with("npc rename ") {
        let name = trimmed[10..].trim();
        if name.is_empty() {
            return Ok(Some(ok_response("npc name cannot be empty.".to_string(), None)));
        }
        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.clone()
        }
        .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;
        draft.name = name.to_string();
        {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::Npc;
            editor.npc_draft = Some(draft.clone());
            editor.location_draft = None;
        }
        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered.starts_with("npc set ") {
        let mut parts = trimmed.splitn(4, char::is_whitespace);
        let _ = parts.next();
        let _ = parts.next();
        let field = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default().trim();
        if value.is_empty() {
            return Ok(Some(ok_response(
                "npc set value cannot be empty.".to_string(),
                None,
            )));
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.clone()
        }
        .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

        let Some(canonical) = canonical_npc_set_field(field) else {
            return Ok(Some(ok_response(
                format!(
                    "unknown npc field: {}. valid fields: name, race, occupation, sex, age, height, weight, background, want, secret, carrying",
                    field
                ),
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

        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered.starts_with("npc travel ") {
        if !lowered.starts_with("npc travel to ") {
            return Ok(Some(ok_response(
                "usage: npc travel to <location>".to_string(),
                None,
            )));
        }
        let location_name = trimmed[14..].trim();
        if location_name.is_empty() {
            return Ok(Some(ok_response("location cannot be empty.".to_string(), None)));
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.clone()
        }
        .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

        let result = ensure_location_exists(
            EnsureLocationInput {
                name: location_name.to_string(),
            },
            state.clone(),
        )
        .await?;
        draft.location = if result.name.trim().is_empty() {
            location_name.to_string()
        } else {
            result.name
        };

        {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::Npc;
            editor.npc_draft = Some(draft.clone());
            editor.location_draft = None;
        }

        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered == "npc save" || lowered == "save" {
        let mode = {
            let editor = state.editor_session.lock().await;
            editor.mode
        };

        if mode == EditorMode::Npc {
            let draft = {
                let editor = state.editor_session.lock().await;
                editor.npc_draft.clone()
            }
            .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

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
            )
            .await?;

            {
                let mut editor = state.editor_session.lock().await;
                editor.mode = EditorMode::None;
                editor.npc_draft = None;
                editor.location_draft = None;
            }

            let output = [
                "## NPC saved".to_string(),
                format!("id: {}", result.id),
                format!("slug: {}", result.slug),
                format!("vault: {}", result.vault_path),
                format!("updated: {}", result.updated_at),
            ]
            .join("\n");

            return Ok(Some(ok_response(output, Some(CommandClientEvent::ClearDrafts))));
        }

        if mode == EditorMode::Location {
            let draft = {
                let editor = state.editor_session.lock().await;
                editor.location_draft.clone()
            }
            .ok_or_else(|| "no active location draft. run load <name>.".to_string())?;

            let result = save_location_draft(
                SaveLocationDraftInput {
                    id: draft.id.clone(),
                    name: draft.name.clone(),
                    slug: draft.slug.clone(),
                    vault_path: draft.vault_path.clone(),
                },
                state.clone(),
            )
            .await?;

            {
                let mut editor = state.editor_session.lock().await;
                editor.mode = EditorMode::None;
                editor.npc_draft = None;
                editor.location_draft = None;
            }

            let output = [
                "## Location saved".to_string(),
                format!("id: {}", result.id),
                format!("slug: {}", result.slug),
                format!("vault: {}", result.vault_path),
                format!("updated: {}", result.updated_at),
            ]
            .join("\n");

            return Ok(Some(ok_response(output, Some(CommandClientEvent::ClearDrafts))));
        }
    }

    if lowered.starts_with("npc reroll ") {
        let args = trimmed[11..].trim();
        if args.is_empty() {
            return Ok(Some(ok_response(
                "usage: npc reroll <field> [prompt]".to_string(),
                None,
            )));
        }
        let mut split = args.splitn(2, char::is_whitespace);
        let field = split.next().unwrap_or_default().trim().to_string();
        let prompt = split.next().map(|value| value.trim().to_string());

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.clone()
        }
        .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

        let rerolled = reroll_npc_field(
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
            state.clone(),
        )
        .await?;

        match rerolled.field.as_str() {
            "name" => {
                if let Some(value) = rerolled.value {
                    draft.name = value;
                }
            }
            "race" => {
                if let Some(value) = rerolled.value {
                    draft.race = value;
                }
            }
            "occupation" => {
                if let Some(value) = rerolled.value {
                    draft.occupation = value;
                }
            }
            "sex" => {
                if let Some(value) = rerolled.value {
                    draft.sex = normalize_sex(&value)?;
                }
            }
            "age" => {
                if let Some(value) = rerolled.value {
                    draft.age = value;
                }
            }
            "height" => {
                if let Some(value) = rerolled.value {
                    draft.height = value;
                }
            }
            "weight_lbs" => {
                if let Some(value) = rerolled.value {
                    draft.weight_lbs = value;
                }
            }
            "background" => {
                if let Some(value) = rerolled.value {
                    draft.background = value;
                }
            }
            "want_need" => {
                if let Some(value) = rerolled.value {
                    draft.want_need = value;
                }
            }
            "secret_obstacle" => {
                if let Some(value) = rerolled.value {
                    draft.secret_obstacle = value;
                }
            }
            "carrying" => {
                if let Some(carrying) = rerolled.carrying {
                    draft.carrying = carrying;
                }
            }
            _ => {}
        }

        {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::Npc;
            editor.npc_draft = Some(draft.clone());
            editor.location_draft = None;
        }

        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered.starts_with("location rename ") {
        let name = trimmed[16..].trim();
        if name.is_empty() {
            return Ok(Some(ok_response(
                "location name cannot be empty.".to_string(),
                None,
            )));
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.location_draft.clone()
        }
        .ok_or_else(|| "no active location draft. run load <name>.".to_string())?;
        draft.name = name.to_string();

        {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::Location;
            editor.location_draft = Some(draft.clone());
            editor.npc_draft = None;
        }

        return Ok(Some(ok_response(
            location_summary_text(&draft),
            Some(location_event_from_draft(&draft)),
        )));
    }

    if lowered.starts_with("npc ") {
        return Ok(Some(ok_response("unknown npc command.".to_string(), None)));
    }

    if lowered.starts_with("location ") {
        return Ok(Some(ok_response(
            "unknown location command.".to_string(),
            None,
        )));
    }

    if lowered == "load" {
        return Ok(Some(ok_response(
            "usage: load <npc-or-location-name>".to_string(),
            None,
        )));
    }

    if lowered.starts_with("load ") {
        let target = trimmed[4..].trim();
        if target.is_empty() {
            return Ok(Some(ok_response(
                "usage: load <npc-or-location-name>".to_string(),
                None,
            )));
        }

        let entity = resolve_entity(target.to_string()).await?;
        let Some(entity) = entity else {
            return Ok(Some(ok_response(
                format!("no npc or location found for: {target}"),
                None,
            )));
        };

        let (output, event) = build_load_response(entity, state.clone()).await;

        return Ok(Some(ok_response(output, event)));
    }

    if lowered == "delete" {
        return Ok(Some(ok_response(
            "usage: delete <npc-or-location-name>".to_string(),
            None,
        )));
    }

    if lowered.starts_with("delete ") {
        let target = trimmed[6..].trim();
        if target.is_empty() {
            return Ok(Some(ok_response(
                "usage: delete <npc-or-location-name>".to_string(),
                None,
            )));
        }

        let result = soft_delete_entity(
            SoftDeleteEntityInput {
                target: target.to_string(),
            },
            state.clone(),
        )
        .await?;

        let output = [
            "## Deleted".to_string(),
            format!("type: {}", result.entity_type.as_str()),
            format!("name: {}", result.name),
            format!("slug: {}", result.slug),
            format!("trash: {}", result.trash_vault_path),
            "tip: run undo to restore it.".to_string(),
        ]
        .join("\n");

        let should_clear = {
            let editor = state.editor_session.lock().await;
            editor
                .npc_draft
                .as_ref()
                .is_some_and(|draft| draft.id == result.id)
                || editor
                    .location_draft
                    .as_ref()
                    .is_some_and(|draft| draft.id == result.id)
        };

        if should_clear {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::None;
            editor.npc_draft = None;
            editor.location_draft = None;
            return Ok(Some(ok_response(output, Some(CommandClientEvent::ClearDrafts))));
        }

        return Ok(Some(ok_response(output, None)));
    }

    if lowered == "undo" {
        let result = undo_last_soft_delete(state).await?;
        let output = [
            "## Undo complete".to_string(),
            format!("type: {}", result.entity_type.as_str()),
            format!("name: {}", result.name),
            format!("slug: {}", result.slug),
            format!("vault: {}", result.vault_path),
        ]
        .join("\n");
        return Ok(Some(ok_response(output, None)));
    }

    let manifest = dnd_core::command_manifest::command_manifest();
    if !starts_with_known_command_root(trimmed, &manifest) {
        if let Some(entity) = resolve_entity(trimmed.to_string()).await? {
            let (output, event) = build_load_response(entity, state).await;
            return Ok(Some(ok_response(output, event)));
        }
    }

    Ok(None)
}

async fn build_load_response(
    entity: EntityDetails,
    state: State<'_, AppState>,
) -> (String, Option<CommandClientEvent>) {
    match entity.entity_type {
        EntityType::Npc => {
            let draft = NpcDraftSession {
                id: entity.id.clone(),
                name: entity.name.clone(),
                race: entity.race.clone().unwrap_or_else(|| "Unknown".to_string()),
                occupation: entity
                    .occupation
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                sex: normalize_sex(
                    &entity
                        .sex
                        .clone()
                        .unwrap_or_else(|| "male".to_string()),
                )
                .unwrap_or_else(|_| "male".to_string()),
                age: entity.age.clone().unwrap_or_else(|| "Unknown".to_string()),
                height: entity.height.clone().unwrap_or_else(|| "Unknown".to_string()),
                weight_lbs: entity
                    .weight_lbs
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                background: entity
                    .background
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                want_need: entity
                    .want_need
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                secret_obstacle: entity
                    .secret_obstacle
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                carrying: entity
                    .carrying
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()]),
                location: entity
                    .location
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            };
            {
                let mut editor = state.editor_session.lock().await;
                editor.mode = EditorMode::Npc;
                editor.location_draft = None;
                editor.npc_draft = Some(draft.clone());
            }

            let carrying = entity
                .carrying
                .as_ref()
                .map(|items| items.join(", "))
                .unwrap_or_else(|| "Unknown".to_string());
            (
                format!(
                    "## NPC\nname: {}\nslug: {}\nrace: {}\noccupation: {}\nsex: {}\nage: {}\nheight: {}\nweight: {}\nbackground: {}\nwant: {}\nsecret: {}\ncarrying: {}\nlocation: {}\npath: {}",
                    entity.name,
                    entity.slug,
                    entity.race.clone().unwrap_or_else(|| "Unknown".to_string()),
                    entity
                        .occupation
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    entity.sex.clone().unwrap_or_else(|| "Unknown".to_string()),
                    entity.age.clone().unwrap_or_else(|| "Unknown".to_string()),
                    entity.height.clone().unwrap_or_else(|| "Unknown".to_string()),
                    entity
                        .weight_lbs
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    entity
                        .background
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    entity
                        .want_need
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    entity
                        .secret_obstacle
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    carrying,
                    entity
                        .location
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    entity.vault_path
                ),
                Some(npc_event_from_draft(&draft)),
            )
        }
        EntityType::Location => {
            let draft = LocationDraftSession {
                id: entity.id.clone(),
                name: entity.name.clone(),
                slug: entity.slug.clone(),
                vault_path: entity.vault_path.clone(),
            };
            {
                let mut editor = state.editor_session.lock().await;
                editor.mode = EditorMode::Location;
                editor.npc_draft = None;
                editor.location_draft = Some(draft.clone());
            }

            (
                format!(
                    "## Location\nname: {}\nslug: {}\npath: {}",
                    entity.name, entity.slug, entity.vault_path
                ),
                Some(location_event_from_draft(&draft)),
            )
        }
    }
}

fn npc_event_from_draft(draft: &NpcDraftSession) -> CommandClientEvent {
    CommandClientEvent::LoadNpcDraft {
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
    }
}

fn location_event_from_draft(draft: &LocationDraftSession) -> CommandClientEvent {
    CommandClientEvent::LoadLocationDraft {
        id: draft.id.clone(),
        name: draft.name.clone(),
        slug: draft.slug.clone(),
        vault_path: draft.vault_path.clone(),
    }
}

fn npc_summary_text(draft: &NpcDraftSession) -> String {
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

fn location_summary_text(draft: &LocationDraftSession) -> String {
    format!(
        "## Location Draft\nname: {}\nslug: {}\npath: {}",
        draft.name, draft.slug, draft.vault_path
    )
}

fn canonical_npc_set_field(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "name" => Some("name"),
        "race" => Some("race"),
        "occupation" => Some("occupation"),
        "sex" => Some("sex"),
        "age" => Some("age"),
        "height" => Some("height"),
        "weight" | "weight_lbs" => Some("weight_lbs"),
        "background" => Some("background"),
        "want" | "need" | "want_need" => Some("want_need"),
        "secret" | "obstacle" | "secret_obstacle" => Some("secret_obstacle"),
        "carrying" => Some("carrying"),
        _ => None,
    }
}

fn ok_response(output: String, client_event: Option<CommandClientEvent>) -> CommandResponse {
    CommandResponse {
        ok: true,
        output: output.clone(),
        error: None,
        exit_code: 0,
        segments: vec![OutputSegment {
            kind: OutputSegmentKind::Text,
            text: output,
            command_ref: None,
        }],
        output_doc: None,
        client_event,
    }
}

fn render_history_output(history: &[String], limit: usize) -> String {
    if history.is_empty() {
        return "(no history)".to_string();
    }

    let safe_limit = limit.clamp(1, 50);
    let start = history.len().saturating_sub(safe_limit);
    history[start..]
        .iter()
        .enumerate()
        .map(|(index, value)| format!("{}: {}", start + index + 1, value))
        .collect::<Vec<_>>()
        .join("\n")
}
