use crate::app_state::{AppState, EditorMode};
use crate::commands::{ok_response, DesktopHandlerInvocation};
use dnd_core::command::CommandClientEvent;
use runebound_models::CommandResponse;

use crate::utils::{
    faction_context_summary, normalize_faction_seed, validate_faction_details,
    reroll_faction_field, FactionRerollContext, RerollFactionFieldInput,
};
use crate::app_state::FactionDraftSession;

pub async fn handle_faction(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "faction help" {
        let has_draft = {
            let editor = invocation.state.editor_session.lock().await;
            editor.faction_draft.is_some()
        };
        if !has_draft {
            return Ok(Some(ok_response("no active faction draft. run create faction or load <name>.".to_string(), None)));
        }
        return Ok(Some(ok_response(
            [
                "## Faction editor commands",
                "faction show",
                "faction rename <name>",
                "faction set <field> <value>",
                "faction reroll <field> [prompt]",
                "reroll",
                "faction save",
                "faction cancel",
            ]
            .join("\n"),
            None,
        )));
    }

    if lowered == "faction show" {
        let draft = {
            let editor = invocation.state.editor_session.lock().await;
            editor.faction_draft.clone()
        };
        let Some(draft) = draft else {
            return Ok(Some(ok_response("no active faction draft. run create faction or load <name>.".to_string(), None)));
        };
        return Ok(Some(ok_response(faction_summary_text(&draft), Some(faction_event_from_draft(&draft)))));
    }

    if lowered == "faction cancel" {
        let had_draft = {
            let mut editor = invocation.state.editor_session.lock().await;
            let had = editor.faction_draft.is_some();
            if had {
                editor.faction_draft = None;
                editor.mode = if editor.npc_draft.is_some() {
                    EditorMode::Npc
                } else if editor.location_draft.is_some() {
                    EditorMode::Location
                } else {
                    EditorMode::None
                };
            }
            had
        };
        if !had_draft {
            return Ok(Some(ok_response("no active faction draft. run create faction or load <name>.".to_string(), None)));
        }
        return Ok(Some(ok_response("faction draft discarded.".to_string(), Some(CommandClientEvent::ClearDrafts))));
    }

    if lowered.starts_with("faction rename ") {
        return faction_rename(trimmed, invocation.state.clone()).await;
    }

    if lowered.starts_with("faction set ") {
        return faction_set(trimmed, invocation.state.clone()).await;
    }

    if lowered.starts_with("faction reroll ") {
        return faction_reroll(trimmed, invocation.state.clone()).await;
    }

    if lowered == "faction save" {
        return faction_save(invocation.state.clone()).await;
    }

    Ok(Some(ok_response("unknown faction command. use `faction help`".to_string(), None)))
}

async fn faction_rename(trimmed: &str, state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    let name = trimmed[15..].trim();
    if name.is_empty() {
        return Ok(Some(ok_response("faction name cannot be empty.".to_string(), None)));
    }

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.faction_draft.clone()
    }.ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;
    draft.name = name.to_string();

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Faction;
        editor.faction_draft = Some(draft.clone());
        editor.npc_draft = None;
        editor.location_draft = None;
    }

    Ok(Some(ok_response(faction_summary_text(&draft), Some(faction_event_from_draft(&draft)))))
}

async fn faction_set(trimmed: &str, state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    let mut parts = trimmed.splitn(4, char::is_whitespace);
    let _ = parts.next();
    let _ = parts.next();
    let field = parts.next().unwrap_or_default();
    let value = parts.next().unwrap_or_default().trim();
    if value.is_empty() {
        return Ok(Some(ok_response("faction set value cannot be empty.".to_string(), None)));
    }

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.faction_draft.clone()
    }.ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;

    let field = canonical_faction_reroll_field(field)?;
    match field {
        "name" => draft.name = value.to_string(),
        "kind_type" => {
            draft.kind_type = normalize_faction_kind_type(value)?;
            if draft.kind_type == "other" && draft.kind_custom.is_none() {
                draft.kind_custom = Some("Unknown".to_string());
            }
        }
        "kind_custom" => draft.kind_custom = Some(value.to_string()),
        "public_description" => draft.public_description = value.to_string(),
        "true_agenda" => draft.true_agenda = value.to_string(),
        "methods" => draft.methods = value.to_string(),
        "leadership" => draft.leadership = value.to_string(),
        "headquarters" => draft.headquarters = value.to_string(),
        "sphere_of_influence" => draft.sphere_of_influence = value.to_string(),
        "resources_assets" => draft.resources_assets = value.to_string(),
        "allies" => draft.allies = normalize_unknown_list(parse_list_csv(value)),
        "rivals_enemies" => draft.rivals_enemies = normalize_unknown_list(parse_list_csv(value)),
        "reputation" => draft.reputation = value.to_string(),
        "current_tension" => draft.current_tension = value.to_string(),
        "goals_short_term" => draft.goals_short_term = normalize_unknown_list(parse_list_csv(value)),
        "goals_long_term" => draft.goals_long_term = normalize_unknown_list(parse_list_csv(value)),
        "symbol_description" => draft.symbol_description = value.to_string(),
        _ => {}
    }

    if draft.kind_type == "other" && draft.kind_custom.as_ref().is_none_or(|item| item.trim().is_empty()) {
        return Ok(Some(ok_response("kind_custom is required when kind is other. use faction set kind_custom <value>.".to_string(), None)));
    }
    if draft.kind_type != "other" {
        draft.kind_custom = None;
    }

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Faction;
        editor.faction_draft = Some(draft.clone());
        editor.npc_draft = None;
        editor.location_draft = None;
    }

    Ok(Some(ok_response(faction_summary_text(&draft), Some(faction_event_from_draft(&draft)))))
}

async fn faction_reroll(trimmed: &str, state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    use crate::utils::normalize_optional_prompt;

    if trimmed.eq_ignore_ascii_case("faction reroll") {
        return Ok(Some(ok_response("usage: faction reroll <field> [prompt]".to_string(), None)));
    }
    if trimmed.len() <= 15 {
        return Ok(Some(ok_response("usage: faction reroll <field> [prompt]".to_string(), None)));
    }
    let args = trimmed[15..].trim();
    if args.is_empty() {
        return Ok(Some(ok_response("usage: faction reroll <field> [prompt]".to_string(), None)));
    }
    let mut split = args.splitn(2, char::is_whitespace);
    let field = split.next().unwrap_or_default().trim().to_string();
    let prompt = normalize_optional_prompt(split.next().map(|value| value.to_string()));

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.faction_draft.clone()
    }.ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;

    let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

    let rerolled = reroll_faction_field(
        RerollFactionFieldInput {
            field,
            prompt,
            faction: FactionRerollContext {
                name: draft.name.clone(),
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
        },
        state.clone(),
    ).await?;

    match rerolled.field.as_str() {
        "name" => { if let Some(value) = rerolled.value { draft.name = value; } }
        "kind_type" => {
            if let Some(value) = rerolled.value {
                draft.kind_type = normalize_faction_kind_type(&value)?;
                if draft.kind_type != "other" { draft.kind_custom = None; }
                else if draft.kind_custom.is_none() { draft.kind_custom = Some("Unknown".to_string()); }
            }
        }
        "kind_custom" => { if let Some(value) = rerolled.value { draft.kind_custom = Some(value); } }
        "public_description" => { if let Some(value) = rerolled.value { draft.public_description = value; } }
        "true_agenda" => { if let Some(value) = rerolled.value { draft.true_agenda = value; } }
        "methods" => { if let Some(value) = rerolled.value { draft.methods = value; } }
        "leadership" => { if let Some(value) = rerolled.value { draft.leadership = value; } }
        "headquarters" => { if let Some(value) = rerolled.value { draft.headquarters = value; } }
        "sphere_of_influence" => { if let Some(value) = rerolled.value { draft.sphere_of_influence = value; } }
        "resources_assets" => { if let Some(value) = rerolled.value { draft.resources_assets = value; } }
        "allies" => { if let Some(value) = rerolled.list_value { draft.allies = value; } }
        "rivals_enemies" => { if let Some(value) = rerolled.list_value { draft.rivals_enemies = value; } }
        "reputation" => { if let Some(value) = rerolled.value { draft.reputation = value; } }
        "current_tension" => { if let Some(value) = rerolled.value { draft.current_tension = value; } }
        "goals_short_term" => { if let Some(value) = rerolled.list_value { draft.goals_short_term = value; } }
        "goals_long_term" => { if let Some(value) = rerolled.list_value { draft.goals_long_term = value; } }
        "symbol_description" => { if let Some(value) = rerolled.value { draft.symbol_description = value; } }
        _ => {}
    }

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Faction;
        editor.faction_draft = Some(draft.clone());
        editor.npc_draft = None;
        editor.location_draft = None;
    }

    Ok(Some(ok_response(faction_summary_text(&draft), Some(faction_event_from_draft(&draft)))))
}

async fn faction_save(state: tauri::State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    use crate::utils::{save_faction_draft_impl, SaveFactionDraftInput, SaveFactionDraftResult};

    let draft = {
        let editor = state.editor_session.lock().await;
        editor.faction_draft.clone()
    }.ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;

    let result = save_faction_draft_impl(
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

pub fn canonical_faction_reroll_field(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    let field = match normalized.as_str() {
        "name" => "name",
        "kind" | "kind_type" => "kind_type",
        "kind_custom" => "kind_custom",
        "public" | "public_description" => "public_description",
        "agenda" | "true_agenda" => "true_agenda",
        "methods" => "methods",
        "leadership" => "leadership",
        "hq" | "headquarters" => "headquarters",
        "influence" | "sphere_of_influence" => "sphere_of_influence",
        "resources" | "resources_assets" => "resources_assets",
        "allies" => "allies",
        "rivals" | "rivals_enemies" => "rivals_enemies",
        "reputation" => "reputation",
        "tension" | "current_tension" => "current_tension",
        "goals_short" | "goals_short_term" => "goals_short_term",
        "goals_long" | "goals_long_term" => "goals_long_term",
        "symbol" | "sigil" | "banner" | "symbol_description" => "symbol_description",
        _ => return Err(format!("unknown faction reroll field: {}. valid fields: name, kind, kind_custom, public, agenda, methods, leadership, headquarters, influence, resources, allies, rivals, reputation, tension, goals_short, goals_long, symbol", raw)),
    };
    Ok(field)
}

pub fn normalize_faction_kind_type(value: &str) -> Result<String, String> {
    const FACTION_KIND_TYPES: [&str; 10] = ["guild", "cult", "military_order", "noble_house", "criminal_syndicate", "mercantile_league", "religious_order", "arcane_circle", "revolutionary_cell", "other"];
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    if FACTION_KIND_TYPES.contains(&normalized.as_str()) { Ok(normalized) }
    else { Err(format!("kind_type must be one of: {}", FACTION_KIND_TYPES.join(", "))) }
}

pub fn normalize_unknown_list(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values.into_iter().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).collect();
    if cleaned.is_empty() { vec!["Unknown".to_string()] } else { cleaned }
}

pub fn parse_list_csv(value: &str) -> Vec<String> {
    value.split(',').map(|item| item.trim().to_string()).filter(|item| !item.is_empty()).collect()
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

pub fn faction_summary_text(draft: &FactionDraftSession) -> String {
    format!(
        "## Faction Draft\nname: {}\nslug: {}\nkind: {}\nkind_custom: {}\npublic: {}\nagenda: {}\nmethods: {}\nleadership: {}\nheadquarters: {}\ninfluence: {}\nresources: {}\nallies: {}\nrivals: {}\nreputation: {}\ntension: {}\ngoals_short: {}\ngoals_long: {}\nsymbol: {}\npath: {}",
        draft.name, draft.slug, draft.kind_type, draft.kind_custom.as_deref().unwrap_or("(none)"),
        draft.public_description, draft.true_agenda, draft.methods, draft.leadership, draft.headquarters,
        draft.sphere_of_influence, draft.resources_assets, draft.allies.join(", "), draft.rivals_enemies.join(", "),
        draft.reputation, draft.current_tension, draft.goals_short_term.join(", "), draft.goals_long_term.join(", "),
        draft.symbol_description, draft.vault_path
    )
}

pub fn faction_event_from_draft(draft: &FactionDraftSession) -> CommandClientEvent {
    use runebound_models::drafts::faction_entity_card;
    use dnd_core::npc::normalize_unknown_text as core_normalize_unknown;
    use dnd_core::npc::normalize_unknown_list as core_normalize_list;

    let normalized_draft = FactionDraftSession {
        id: draft.id.clone(), name: draft.name.clone(), slug: draft.slug.clone(), vault_path: draft.vault_path.clone(),
        kind_type: draft.kind_type.clone(), kind_custom: draft.kind_custom.clone(),
        public_description: core_normalize_unknown(&draft.public_description),
        true_agenda: core_normalize_unknown(&draft.true_agenda),
        methods: core_normalize_unknown(&draft.methods),
        leadership: core_normalize_unknown(&draft.leadership),
        headquarters: core_normalize_unknown(&draft.headquarters),
        sphere_of_influence: core_normalize_unknown(&draft.sphere_of_influence),
        resources_assets: core_normalize_unknown(&draft.resources_assets),
        allies: core_normalize_list(draft.allies.clone()),
        rivals_enemies: core_normalize_list(draft.rivals_enemies.clone()),
        reputation: core_normalize_unknown(&draft.reputation),
        current_tension: core_normalize_unknown(&draft.current_tension),
        goals_short_term: core_normalize_list(draft.goals_short_term.clone()),
        goals_long_term: core_normalize_list(draft.goals_long_term.clone()),
        symbol_description: core_normalize_unknown(&draft.symbol_description),
        seed_prompt: draft.seed_prompt.clone(),
    };
    let entity_card_doc = faction_entity_card(&normalized_draft);
    CommandClientEvent::LoadFactionDraftWithCard { draft: normalized_draft, entity_card: entity_card_doc }
}

pub fn path_for_display(path: &str) -> String {
    if std::path::MAIN_SEPARATOR == '\\' { path.replace('/', "\\") } else { path.replace('\\', "/") }
}
