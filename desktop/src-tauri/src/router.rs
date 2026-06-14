use dnd_core::command::{CommandClientEvent, CommandResponse, OutputSegment, OutputSegmentKind};
use dnd_core::output::{OutputDoc, entity_card, entity_row};
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
            [
                "## Create commands",
                "create npc",
                "create npc <prompt text>",
                "create location",
                "create location <prompt text>",
            ]
            .join("\n"),
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

    if lowered == "create location" || lowered.starts_with("create location ") {
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

        let seed = generate_location_seed(GenerateLocationSeedInput { prompt }, state.clone()).await?;
        let draft = LocationDraftSession {
            id: make_entity_id("loc"),
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
            editor.mode = EditorMode::Location;
            editor.npc_draft = None;
            editor.location_draft = Some(draft.clone());
        }

        return Ok(Some(ok_response(
            location_summary_text(&draft),
            Some(location_event_from_draft(&draft)),
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
                "no active location draft. run create location or load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            [
                "## Location editor commands",
                "location show",
                "location rename <name>",
                "location set <field> <value>",
                "location reroll <field> [prompt]",
                "reroll",
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
                "no active location draft. run create location or load <name>.".to_string(),
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
                "no active location draft. run create location or load <name>.".to_string(),
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
        let mode = {
            let editor = state.editor_session.lock().await;
            editor.mode
        };

        if mode == EditorMode::Npc {
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

        if mode == EditorMode::Location {
            let draft = {
                let editor = state.editor_session.lock().await;
                editor.location_draft.clone()
            };
            let Some(mut draft) = draft else {
                return Ok(None);
            };

            let seed =
                generate_location_seed(GenerateLocationSeedInput { prompt: None }, state.clone())
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
                editor.mode = EditorMode::Location;
                editor.npc_draft = None;
                editor.location_draft = Some(draft.clone());
            }

            return Ok(Some(ok_response(
                location_summary_text(&draft),
                Some(location_event_from_draft(&draft)),
            )));
        }

        return Ok(None);
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
                format!("vault: {}", path_for_display(&result.vault_path)),
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
            .ok_or_else(|| {
                "no active location draft. run create location or load <name>.".to_string()
            })?;

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
                format!("vault: {}", path_for_display(&result.vault_path)),
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
        .ok_or_else(|| "no active location draft. run create location or load <name>.".to_string())?;
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

    if lowered.starts_with("location set ") {
        let mut parts = trimmed.splitn(4, char::is_whitespace);
        let _ = parts.next();
        let _ = parts.next();
        let field = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default().trim();
        if value.is_empty() {
            return Ok(Some(ok_response(
                "location set value cannot be empty.".to_string(),
                None,
            )));
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.location_draft.clone()
        }
        .ok_or_else(|| "no active location draft. run create location or load <name>.".to_string())?;

        let Some(canonical) = canonical_location_set_field(field) else {
            return Ok(Some(ok_response(
                format!(
                    "unknown location field: {}. valid fields: name, kind, kind_custom, visual, history, exports, tone, authority, danger, tension",
                    field
                ),
                None,
            )));
        };

        match canonical {
            "name" => draft.name = value.to_string(),
            "kind_type" => {
                draft.kind_type = normalize_location_kind_type(value)?;
                if draft.kind_type == "other" && draft.kind_custom.is_none() {
                    draft.kind_custom = Some("Unknown".to_string());
                }
            }
            "kind_custom" => draft.kind_custom = Some(value.to_string()),
            "visual_description" => draft.visual_description = value.to_string(),
            "history_background" => draft.history_background = value.to_string(),
            "exports" => draft.exports = normalize_exports(parse_list_csv(value)),
            "tone" => draft.tone = value.to_string(),
            "authority" => draft.authority = value.to_string(),
            "danger_level" => draft.danger_level = normalize_location_danger_level(value)?,
            "current_tension" => draft.current_tension = value.to_string(),
            _ => {}
        }

        if draft.kind_type == "other"
            && draft
                .kind_custom
                .as_ref()
                .is_none_or(|item| item.trim().is_empty())
        {
            return Ok(Some(ok_response(
                "kind_custom is required when kind is other. use location set kind_custom <value>."
                    .to_string(),
                None,
            )));
        }
        if draft.kind_type != "other" {
            draft.kind_custom = None;
        }

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

    if lowered.starts_with("location reroll ") {
        let args = trimmed[16..].trim();
        if args.is_empty() {
            return Ok(Some(ok_response(
                "usage: location reroll <field> [prompt]".to_string(),
                None,
            )));
        }
        let mut split = args.splitn(2, char::is_whitespace);
        let field = split.next().unwrap_or_default().trim().to_string();
        let prompt = split.next().map(|value| value.trim().to_string());

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.location_draft.clone()
        }
        .ok_or_else(|| "no active location draft. run create location or load <name>.".to_string())?;

        let rerolled = reroll_location_field(
            RerollLocationFieldInput {
                field,
                prompt,
                location: LocationRerollContext {
                    name: draft.name.clone(),
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
            "kind_type" => {
                if let Some(value) = rerolled.value {
                    draft.kind_type = normalize_location_kind_type(&value)?;
                    if draft.kind_type != "other" {
                        draft.kind_custom = None;
                    } else if draft.kind_custom.is_none() {
                        draft.kind_custom = Some("Unknown".to_string());
                    }
                }
            }
            "kind_custom" => {
                if let Some(value) = rerolled.value {
                    draft.kind_custom = Some(value);
                }
            }
            "visual_description" => {
                if let Some(value) = rerolled.value {
                    draft.visual_description = value;
                }
            }
            "history_background" => {
                if let Some(value) = rerolled.value {
                    draft.history_background = value;
                }
            }
            "exports" => {
                if let Some(exports) = rerolled.exports {
                    draft.exports = exports;
                }
            }
            "tone" => {
                if let Some(value) = rerolled.value {
                    draft.tone = value;
                }
            }
            "authority" => {
                if let Some(value) = rerolled.value {
                    draft.authority = value;
                }
            }
            "danger_level" => {
                if let Some(value) = rerolled.value {
                    draft.danger_level = normalize_location_danger_level(&value)?;
                }
            }
            "current_tension" => {
                if let Some(value) = rerolled.value {
                    draft.current_tension = value;
                }
            }
            _ => {}
        }

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

    if lowered == "show" || lowered == "preview" {
        return Ok(Some(ok_response(
            "usage: show <npc-or-location-name>".to_string(),
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

    if lowered.starts_with("show ") || lowered.starts_with("preview ") {
        let target = if lowered.starts_with("show ") {
            trimmed[4..].trim()
        } else {
            trimmed[7..].trim()
        };
        if target.is_empty() {
            return Ok(Some(ok_response(
                "usage: show <npc-or-location-name>".to_string(),
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

        let preview_text = build_preview_response(entity.clone());
        let preview_doc = build_entity_card_doc(&entity);
        return Ok(Some(ok_response_with_doc(preview_text, Some(preview_doc), None)));
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
            format!("trash: {}", path_for_display(&result.trash_vault_path)),
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
            format!("vault: {}", path_for_display(&result.vault_path)),
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

            (
                build_entity_card_text(&entity),
                Some(npc_event_from_draft(&draft)),
            )
        }
        EntityType::Location => {
            let draft = LocationDraftSession {
                id: entity.id.clone(),
                name: entity.name.clone(),
                slug: entity.slug.clone(),
                vault_path: path_for_display(&entity.vault_path),
                kind_type: entity
                    .kind_type
                    .clone()
                    .unwrap_or_else(|| "other".to_string()),
                kind_custom: entity.kind_custom.clone(),
                visual_description: entity
                    .visual_description
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                history_background: entity
                    .history_background
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                exports: entity
                    .exports
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()]),
                tone: entity.tone.clone().unwrap_or_else(|| "Unknown".to_string()),
                authority: entity.authority.clone().unwrap_or_else(|| "Unknown".to_string()),
                danger_level: entity
                    .danger_level
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                current_tension: entity
                    .current_tension
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            };
            {
                let mut editor = state.editor_session.lock().await;
                editor.mode = EditorMode::Location;
                editor.npc_draft = None;
                editor.location_draft = Some(draft.clone());
            }

            (build_entity_card_text(&entity), Some(location_event_from_draft(&draft)))
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
            rows.push(entity_row(
                "race",
                entity.race.clone().unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "occupation",
                entity
                    .occupation
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "sex",
                entity.sex.clone().unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "age",
                entity.age.clone().unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "height",
                entity.height.clone().unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "weight",
                entity
                    .weight_lbs
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "background",
                entity
                    .background
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "want",
                entity
                    .want_need
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "secret",
                entity
                    .secret_obstacle
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "carrying",
                entity
                    .carrying
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()])
                    .join(", "),
            ));
            rows.push(entity_row(
                "location",
                entity
                    .location
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row("path", path_for_display(&entity.vault_path)));

            OutputDoc {
                blocks: vec![entity_card("NPC", rows)],
            }
        }
        EntityType::Location => {
            rows.push(entity_row(
                "kind",
                entity
                    .kind_type
                    .clone()
                    .unwrap_or_else(|| "other".to_string()),
            ));
            rows.push(entity_row(
                "kind_custom",
                entity
                    .kind_custom
                    .clone()
                    .unwrap_or_else(|| "(none)".to_string()),
            ));
            rows.push(entity_row(
                "visual",
                entity
                    .visual_description
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "history",
                entity
                    .history_background
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "exports",
                entity
                    .exports
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()])
                    .join(", "),
            ));
            rows.push(entity_row(
                "tone",
                entity.tone.clone().unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "authority",
                entity
                    .authority
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "danger",
                entity
                    .danger_level
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "tension",
                entity
                    .current_tension
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row("path", path_for_display(&entity.vault_path)));

            OutputDoc {
                blocks: vec![entity_card("Location", rows)],
            }
        }
    }
}

fn build_entity_card_text(entity: &EntityDetails) -> String {
    match entity.entity_type {
        EntityType::Npc => {
            let carrying = entity
                .carrying
                .as_ref()
                .map(|items| items.join(", "))
                .unwrap_or_else(|| "Unknown".to_string());
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
                path_for_display(&entity.vault_path)
            )
        }
        EntityType::Location => {
            let kind_type = entity
                .kind_type
                .clone()
                .unwrap_or_else(|| "other".to_string());
            let kind_custom = entity
                .kind_custom
                .clone()
                .unwrap_or_else(|| "(none)".to_string());
            let visual_description = entity
                .visual_description
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let history_background = entity
                .history_background
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let exports = entity
                .exports
                .clone()
                .unwrap_or_else(|| vec!["Unknown".to_string()])
                .join(", ");
            let tone = entity.tone.clone().unwrap_or_else(|| "Unknown".to_string());
            let authority = entity
                .authority
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let danger_level = entity
                .danger_level
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let current_tension = entity
                .current_tension
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());

            format!(
                "## Location\nname: {}\nslug: {}\nkind: {}\nkind_custom: {}\nvisual: {}\nhistory: {}\nexports: {}\ntone: {}\nauthority: {}\ndanger: {}\ntension: {}\npath: {}",
                entity.name,
                entity.slug,
                kind_type,
                kind_custom,
                visual_description,
                history_background,
                exports,
                tone,
                authority,
                danger_level,
                current_tension,
                path_for_display(&entity.vault_path)
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
        kind_type: draft.kind_type.clone(),
        kind_custom: draft.kind_custom.clone(),
        visual_description: draft.visual_description.clone(),
        history_background: draft.history_background.clone(),
        exports: draft.exports.clone(),
        tone: draft.tone.clone(),
        authority: draft.authority.clone(),
        danger_level: draft.danger_level.clone(),
        current_tension: draft.current_tension.clone(),
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
        "## Location Draft\nname: {}\nslug: {}\nkind: {}\nkind_custom: {}\nvisual: {}\nhistory: {}\nexports: {}\ntone: {}\nauthority: {}\ndanger: {}\ntension: {}\npath: {}",
        draft.name,
        draft.slug,
        draft.kind_type,
        draft.kind_custom.as_deref().unwrap_or("(none)"),
        draft.visual_description,
        draft.history_background,
        draft.exports.join(", "),
        draft.tone,
        draft.authority,
        draft.danger_level,
        draft.current_tension,
        draft.vault_path
    )
}

fn canonical_location_set_field(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "name" => Some("name"),
        "kind" | "kind_type" => Some("kind_type"),
        "kind_custom" | "custom_kind" => Some("kind_custom"),
        "visual" | "visual_description" | "description" => Some("visual_description"),
        "history" | "history_background" | "background" => Some("history_background"),
        "exports" => Some("exports"),
        "tone" => Some("tone"),
        "authority" => Some("authority"),
        "danger" | "danger_level" => Some("danger_level"),
        "tension" | "current_tension" => Some("current_tension"),
        _ => None,
    }
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
    ok_response_with_doc(output, None, client_event)
}

fn ok_response_with_doc(
    output: String,
    output_doc: Option<OutputDoc>,
    client_event: Option<CommandClientEvent>,
) -> CommandResponse {
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
        output_doc,
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
