use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use command_handler::{CommandHandler, HandlerBridge, HandlerEntry, HandlerMetadata, HandlerRegistry};
use dnd_core::command::{CommandClientEvent, CommandResponse, OutputSegment, OutputSegmentKind};
use dnd_core::output::{OutputDoc, entity_card, entity_row};
use tauri::State;

use crate::app_state::{
    AppState, EditorMode, FactionDraftSession, LocationDraftSession, NpcDraftSession,
};
use command_specs::handler_metadata_for;

use super::*;

type DesktopHandlerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<CommandResponse>, String>> + Send + 'a>>;
struct DesktopHandler {
    inner:
        Arc<dyn for<'a> Fn(DesktopHandlerInvocation<'a>) -> DesktopHandlerFuture<'a> + Send + Sync>,
}

impl DesktopHandler {
    fn new<F>(handler: F) -> Self
    where
        F: for<'a> Fn(DesktopHandlerInvocation<'a>) -> DesktopHandlerFuture<'a> + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(handler),
        }
    }
}

impl HandlerBridge for DesktopHandler {
    type Output = Result<Option<CommandResponse>, String>;
    type Invocation<'a> = DesktopHandlerInvocation<'a>;

    fn invoke<'a>(&'a self, invocation: Self::Invocation<'a>) -> command_handler::HandlerFuture<'a, Self::Output> {
        (self.inner)(invocation)
    }
}

pub struct DesktopHandlerInvocation<'a> {
    pub raw_input: &'a str,
    pub tokens: &'a [String],
    pub lowered: &'a [String],
    pub state: State<'a, AppState>,
}

fn desktop_handler_registry() -> &'static HandlerRegistry<DesktopHandler> {
    static REGISTRY: OnceLock<HandlerRegistry<DesktopHandler>> = OnceLock::new();
    REGISTRY.get_or_init(build_desktop_handler_registry)
}

fn build_desktop_handler_registry() -> HandlerRegistry<DesktopHandler> {
    let mut registry = HandlerRegistry::new();
    registry.register(exit_handler_entry());
    registry.register(clear_handler_entry());
    registry.register(history_handler_entry());
    registry.register(create_handler_entry());
    registry.register(npc_handler_entry());
    registry.register(location_handler_entry());
    registry.register(faction_handler_entry());
    registry.register(load_handler_entry());
    registry.register(show_handler_entry());
    registry.register(preview_handler_entry());
    registry.register(delete_handler_entry());
    registry.register(undo_handler_entry());
    registry.register(save_handler_entry());
    registry.register(reroll_handler_entry());
    registry.register(cancel_handler_entry());
    registry
}

fn metadata_for(name: &str) -> HandlerMetadata {
    handler_metadata_for(name)
        .unwrap_or_else(|| panic!("missing handler metadata for {name}"))
        .into()
}

pub(crate) async fn dispatch_desktop_command(
    input: &str,
    tokens: &[String],
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    if tokens.is_empty() {
        return Ok(None);
    }

    let lowered: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();

    let registry = desktop_handler_registry();
    if let Some(entry) = registry.get(lowered[0].as_str()) {
        let invocation = DesktopHandlerInvocation {
            raw_input: input,
            tokens,
            lowered: &lowered,
            state,
        };
        return entry.execute(invocation).await;
    }

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
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

fn clear_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "clear",
        metadata_for("clear"),
        DesktopHandler::new(|invocation| {
            let state = invocation.state.clone();
            Box::pin(async move {
                if invocation.lowered.len() == 1 {
                    return Ok(Some(ok_response(
                        String::new(),
                        Some(CommandClientEvent::ClearTerminal {
                            clear_history: false,
                        }),
                    )));
                }

                if invocation.lowered.len() == 2 && invocation.lowered[1] == "--history" {
                    {
                        let mut service = state.command_service.lock().await;
                        service.session_mut().clear_history();
                    }
                    return Ok(Some(ok_response(
                        String::new(),
                        Some(CommandClientEvent::ClearTerminal {
                            clear_history: true,
                        }),
                    )));
                }

                Ok(Some(ok_response(
                    "usage: clear [--history]".to_string(),
                    None,
                )))
            })
        }),
    )
}

fn history_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "history",
        metadata_for("history"),
        DesktopHandler::new(|invocation| {
            let state = invocation.state.clone();
            Box::pin(async move {
                if invocation.lowered.len() >= 2 && invocation.lowered[1] == "clear" {
                    {
                        let mut service = state.command_service.lock().await;
                        service.session_mut().clear_history();
                    }
                    return Ok(Some(ok_response("history cleared".to_string(), None)));
                }

                if invocation.lowered.len() > 2 {
                    return Ok(Some(ok_response(
                        "usage: history [limit|clear]".to_string(),
                        None,
                    )));
                }

                let limit = if invocation.lowered.len() == 2 {
                    match invocation.lowered[1].parse::<usize>() {
                        Ok(parsed) if parsed > 0 => parsed,
                        _ => {
                            return Ok(Some(ok_response(
                                "usage: history [limit|clear]".to_string(),
                                None,
                            )))
                        }
                    }
                } else {
                    20
                };

                let history = {
                    let service = state.command_service.lock().await;
                    service.session().command_history.clone()
                };
                Ok(Some(ok_response(render_history_output(&history, limit), None)))
            })
        }),
    )
}

fn create_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "create",
        metadata_for("create"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_create(invocation).await })),
    )
}

fn exit_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "exit",
        metadata_for("exit"),
        DesktopHandler::new(|_| {
            Box::pin(async {
                Ok(Some(ok_response(
                    "exiting".to_string(),
                    Some(CommandClientEvent::ExitRequested),
                )))
            })
        }),
    )
}

async fn handle_create(
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

fn npc_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "npc",
        metadata_for("npc"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_npc(invocation).await })),
    )
}

fn location_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "location",
        metadata_for("location"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_location(invocation).await })),
    )
}

async fn handle_location(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "location help" {
        let has_draft = {
            let editor = invocation.state.editor_session.lock().await;
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

    if lowered == "location show" {
        let draft = {
            let editor = invocation.state.editor_session.lock().await;
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

    if lowered == "location cancel" {
        let had_draft = {
            let mut editor = invocation.state.editor_session.lock().await;
            let had = editor.location_draft.is_some();
            if had {
                editor.location_draft = None;
                editor.mode = if editor.npc_draft.is_some() {
                    EditorMode::Npc
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
                "no active location draft. run create location or load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            "location draft discarded.".to_string(),
            Some(CommandClientEvent::ClearDrafts),
        )));
    }

    if lowered.starts_with("location rename ") {
        return location_rename(trimmed, invocation.state.clone()).await;
    }

    if lowered.starts_with("location set ") {
        return location_set(trimmed, invocation.state.clone()).await;
    }

    if lowered.starts_with("location reroll ") {
        return location_reroll(trimmed, invocation.state.clone()).await;
    }

    if lowered == "location save" {
        return location_save(invocation.state.clone()).await;
    }

    Ok(Some(ok_response(
        "unknown location command. use `location help`".to_string(),
        None,
    )))
}

async fn location_rename(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
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

    Ok(Some(ok_response(
        location_summary_text(&draft),
        Some(location_event_from_draft(&draft)),
    )))
}

async fn location_set(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
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

    Ok(Some(ok_response(
        location_summary_text(&draft),
        Some(location_event_from_draft(&draft)),
    )))
}

async fn location_reroll(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    if trimmed.eq_ignore_ascii_case("location reroll") {
        return Ok(Some(ok_response(
            "usage: location reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    if trimmed.len() <= 16 {
        return Ok(Some(ok_response(
            "usage: location reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let args = trimmed[16..].trim();
    if args.is_empty() {
        return Ok(Some(ok_response(
            "usage: location reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let mut split = args.splitn(2, char::is_whitespace);
    let field = split.next().unwrap_or_default().trim().to_string();
    let prompt = normalize_optional_prompt(split.next().map(|value| value.to_string()));

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.location_draft.clone()
    }
    .ok_or_else(|| "no active location draft. run create location or load <name>.".to_string())?;

    let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

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

    Ok(Some(ok_response(
        location_summary_text(&draft),
        Some(location_event_from_draft(&draft)),
    )))
}

async fn location_save(state: State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    let draft = {
        let editor = state.editor_session.lock().await;
        editor.location_draft.clone()
    }
    .ok_or_else(|| "no active location draft. run create location or load <name>.".to_string())?;

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
        editor.faction_draft = None;
    }

    let output = [
        "## Location saved".to_string(),
        format!("id: {}", result.id),
        format!("slug: {}", result.slug),
        format!("vault: {}", path_for_display(&result.vault_path)),
        format!("updated: {}", result.updated_at),
    ]
    .join("\n");

    Ok(Some(ok_response(
        output,
        Some(CommandClientEvent::ClearDrafts),
    )))
}

fn faction_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "faction",
        metadata_for("faction"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_faction(invocation).await })),
    )
}

async fn handle_faction(
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
            return Ok(Some(ok_response(
                "no active faction draft. run create faction or load <name>.".to_string(),
                None,
            )));
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
            return Ok(Some(ok_response(
                "no active faction draft. run create faction or load <name>.".to_string(),
                None,
            )));
        };
        return Ok(Some(ok_response(
            faction_summary_text(&draft),
            Some(faction_event_from_draft(&draft)),
        )));
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
            return Ok(Some(ok_response(
                "no active faction draft. run create faction or load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            "faction draft discarded.".to_string(),
            Some(CommandClientEvent::ClearDrafts),
        )));
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

    Ok(Some(ok_response(
        "unknown faction command. use `faction help`".to_string(),
        None,
    )))
}

async fn faction_rename(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    let name = trimmed[15..].trim();
    if name.is_empty() {
        return Ok(Some(ok_response(
            "faction name cannot be empty.".to_string(),
            None,
        )));
    }

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.faction_draft.clone()
    }
    .ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;
    draft.name = name.to_string();

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Faction;
        editor.faction_draft = Some(draft.clone());
        editor.npc_draft = None;
        editor.location_draft = None;
    }

    Ok(Some(ok_response(
        faction_summary_text(&draft),
        Some(faction_event_from_draft(&draft)),
    )))
}

async fn faction_set(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    let mut parts = trimmed.splitn(4, char::is_whitespace);
    let _ = parts.next();
    let _ = parts.next();
    let field = parts.next().unwrap_or_default();
    let value = parts.next().unwrap_or_default().trim();
    if value.is_empty() {
        return Ok(Some(ok_response(
            "faction set value cannot be empty.".to_string(),
            None,
        )));
    }

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.faction_draft.clone()
    }
    .ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;

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
        "rivals_enemies" => {
            draft.rivals_enemies = normalize_unknown_list(parse_list_csv(value))
        }
        "reputation" => draft.reputation = value.to_string(),
        "current_tension" => draft.current_tension = value.to_string(),
        "goals_short_term" => {
            draft.goals_short_term = normalize_unknown_list(parse_list_csv(value))
        }
        "goals_long_term" => {
            draft.goals_long_term = normalize_unknown_list(parse_list_csv(value))
        }
        "symbol_description" => draft.symbol_description = value.to_string(),
        _ => {}
    }

    if draft.kind_type == "other"
        && draft
            .kind_custom
            .as_ref()
            .is_none_or(|item| item.trim().is_empty())
    {
        return Ok(Some(ok_response(
            "kind_custom is required when kind is other. use faction set kind_custom <value>."
                .to_string(),
            None,
        )));
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

    Ok(Some(ok_response(
        faction_summary_text(&draft),
        Some(faction_event_from_draft(&draft)),
    )))
}

async fn faction_reroll(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    if trimmed.eq_ignore_ascii_case("faction reroll") {
        return Ok(Some(ok_response(
            "usage: faction reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    if trimmed.len() <= 15 {
        return Ok(Some(ok_response(
            "usage: faction reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let args = trimmed[15..].trim();
    if args.is_empty() {
        return Ok(Some(ok_response(
            "usage: faction reroll <field> [prompt]".to_string(),
            None,
        )));
    }

    let mut split = args.splitn(2, char::is_whitespace);
    let field = split.next().unwrap_or_default().trim().to_string();
    let prompt = normalize_optional_prompt(split.next().map(|value| value.to_string()));

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.faction_draft.clone()
    }
    .ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;

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
                draft.kind_type = normalize_faction_kind_type(&value)?;
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
        "public_description" => {
            if let Some(value) = rerolled.value {
                draft.public_description = value;
            }
        }
        "true_agenda" => {
            if let Some(value) = rerolled.value {
                draft.true_agenda = value;
            }
        }
        "methods" => {
            if let Some(value) = rerolled.value {
                draft.methods = value;
            }
        }
        "leadership" => {
            if let Some(value) = rerolled.value {
                draft.leadership = value;
            }
        }
        "headquarters" => {
            if let Some(value) = rerolled.value {
                draft.headquarters = value;
            }
        }
        "sphere_of_influence" => {
            if let Some(value) = rerolled.value {
                draft.sphere_of_influence = value;
            }
        }
        "resources_assets" => {
            if let Some(value) = rerolled.value {
                draft.resources_assets = value;
            }
        }
        "allies" => {
            if let Some(value) = rerolled.list_value {
                draft.allies = value;
            }
        }
        "rivals_enemies" => {
            if let Some(value) = rerolled.list_value {
                draft.rivals_enemies = value;
            }
        }
        "reputation" => {
            if let Some(value) = rerolled.value {
                draft.reputation = value;
            }
        }
        "current_tension" => {
            if let Some(value) = rerolled.value {
                draft.current_tension = value;
            }
        }
        "goals_short_term" => {
            if let Some(value) = rerolled.list_value {
                draft.goals_short_term = value;
            }
        }
        "goals_long_term" => {
            if let Some(value) = rerolled.list_value {
                draft.goals_long_term = value;
            }
        }
        "symbol_description" => {
            if let Some(value) = rerolled.value {
                draft.symbol_description = value;
            }
        }
        _ => {}
    }

    {
        let mut editor = state.editor_session.lock().await;
        editor.mode = EditorMode::Faction;
        editor.faction_draft = Some(draft.clone());
        editor.npc_draft = None;
        editor.location_draft = None;
    }

    Ok(Some(ok_response(
        faction_summary_text(&draft),
        Some(faction_event_from_draft(&draft)),
    )))
}

async fn faction_save(state: State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
    let draft = {
        let editor = state.editor_session.lock().await;
        editor.faction_draft.clone()
    }
    .ok_or_else(|| "no active faction draft. run create faction or load <name>.".to_string())?;

    let result = save_faction_draft(
        SaveFactionDraftInput {
            id: draft.id.clone(),
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
        state.clone(),
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
        "## Faction saved".to_string(),
        format!("id: {}", result.id),
        format!("slug: {}", result.slug),
        format!("vault: {}", path_for_display(&result.vault_path)),
        format!("updated: {}", result.updated_at),
    ]
    .join("\n");

    Ok(Some(ok_response(
        output,
        Some(CommandClientEvent::ClearDrafts),
    )))
}

fn load_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "load",
        metadata_for("load"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_load(invocation).await })),
    )
}

async fn handle_load(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "load" {
        return Ok(Some(ok_response(
            "usage: load <npc-or-location-or-faction-name>".to_string(),
            None,
        )));
    }

    if !lowered.starts_with("load ") {
        return Ok(None);
    }

    let target = trimmed[4..].trim();
    if target.is_empty() {
        return Ok(Some(ok_response(
            "usage: load <npc-or-location-or-faction-name>".to_string(),
            None,
        )));
    }

    let entity = resolve_entity(target.to_string()).await?;
    let Some(entity) = entity else {
        return Ok(Some(ok_response(
            format!("no npc, location, or faction found for: {target}"),
            None,
        )));
    };

    let (output, event) = build_load_response(entity, invocation.state.clone()).await;
    Ok(Some(ok_response(output, event)))
}

fn show_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "show",
        metadata_for("show"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_show(invocation).await })),
    )
}

async fn handle_show(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    entity_preview_response(invocation, "show").await
}

fn preview_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "preview",
        metadata_for("preview"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_preview(invocation).await })),
    )
}

async fn handle_preview(
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
        return Ok(Some(ok_response(
            format!("usage: {} <npc-or-location-or-faction-name>", root),
            None,
        )));
    }
    if !lowered.starts_with(&format!("{root} ")) {
        return Ok(None);
    }
    let target = trimmed[root.len()..].trim();
    if target.is_empty() {
        return Ok(Some(ok_response(
            format!("usage: {} <npc-or-location-or-faction-name>", root),
            None,
        )));
    }
    let entity = resolve_entity(target.to_string()).await?;
    let Some(entity) = entity else {
        return Ok(Some(ok_response(
            format!("no npc, location, or faction found for: {target}"),
            None,
        )));
    };

    let preview_text = build_preview_response(entity.clone());
    let preview_doc = build_entity_card_doc(&entity);
    Ok(Some(ok_response_with_doc(
        preview_text,
        Some(preview_doc),
        None,
    )))
}

fn delete_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "delete",
        metadata_for("delete"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_delete(invocation).await })),
    )
}

async fn handle_delete(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    if lowered == "delete" {
        return Ok(Some(ok_response(
            "usage: delete <npc-or-location-or-faction-name>".to_string(),
            None,
        )));
    }
    if !lowered.starts_with("delete ") {
        return Ok(None);
    }
    let target = trimmed[6..].trim();
    if target.is_empty() {
        return Ok(Some(ok_response(
            "usage: delete <npc-or-location-or-faction-name>".to_string(),
            None,
        )));
    }

    let result = soft_delete_entity(
        SoftDeleteEntityInput {
            target: target.to_string(),
        },
        invocation.state.clone(),
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
        let editor = invocation.state.editor_session.lock().await;
        editor
            .npc_draft
            .as_ref()
            .is_some_and(|draft| draft.id == result.id)
            || editor
                .location_draft
                .as_ref()
                .is_some_and(|draft| draft.id == result.id)
            || editor
                .faction_draft
                .as_ref()
                .is_some_and(|draft| draft.id == result.id)
    };

    if should_clear {
        let mut editor = invocation.state.editor_session.lock().await;
        editor.mode = EditorMode::None;
        editor.npc_draft = None;
        editor.location_draft = None;
        editor.faction_draft = None;
        return Ok(Some(ok_response(
            output,
            Some(CommandClientEvent::ClearDrafts),
        )));
    }

    Ok(Some(ok_response(output, None)))
}

fn undo_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "undo",
        metadata_for("undo"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_undo(invocation).await })),
    )
}

fn save_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "save",
        metadata_for("save"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_save(invocation).await })),
    )
}

async fn handle_save(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let mode = {
        let editor = invocation.state.editor_session.lock().await;
        editor.mode
    };

    match mode {
        EditorMode::Npc => npc_save(invocation.state.clone()).await,
        EditorMode::Location => location_save(invocation.state.clone()).await,
        EditorMode::Faction => faction_save(invocation.state.clone()).await,
        EditorMode::None => Ok(Some(ok_response(
            "no active draft to save.".to_string(),
            None,
        ))),
    }
}

fn reroll_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "reroll",
        metadata_for("reroll"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_reroll(invocation).await })),
    )
}

async fn handle_reroll(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let mode = {
        let editor = invocation.state.editor_session.lock().await;
        editor.mode
    };

    match mode {
        EditorMode::Npc => reroll_current_npc(invocation.state.clone()).await,
        EditorMode::Location => reroll_current_location(invocation.state.clone()).await,
        EditorMode::Faction => reroll_current_faction(invocation.state.clone()).await,
        EditorMode::None => Ok(Some(ok_response(
            "no active draft to reroll.".to_string(),
            None,
        ))),
    }
}

async fn reroll_current_npc(
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    let draft = {
        let editor = state.editor_session.lock().await;
        editor.npc_draft.clone()
    };
    let Some(mut draft) = draft else {
        return Ok(Some(ok_response("no active npc draft.".to_string(), None)));
    };

    let seed = generate_npc_seed(
        GenerateNpcSeedInput {
            prompt: draft.seed_prompt.clone(),
        },
        state.clone(),
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
        editor.mode = EditorMode::Npc;
        editor.location_draft = None;
        editor.npc_draft = Some(draft.clone());
    }

    Ok(Some(ok_response(
        npc_summary_text(&draft),
        Some(npc_event_from_draft(&draft)),
    )))
}

async fn reroll_current_location(
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    let draft = {
        let editor = state.editor_session.lock().await;
        editor.location_draft.clone()
    };
    let Some(mut draft) = draft else {
        return Ok(Some(ok_response("no active location draft.".to_string(), None)));
    };

    let seed = generate_location_seed(
        GenerateLocationSeedInput {
            prompt: draft.seed_prompt.clone(),
        },
        state.clone(),
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
        editor.mode = EditorMode::Location;
        editor.npc_draft = None;
        editor.location_draft = Some(draft.clone());
    }

    Ok(Some(ok_response(
        location_summary_text(&draft),
        Some(location_event_from_draft(&draft)),
    )))
}

async fn reroll_current_faction(
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    let draft = {
        let editor = state.editor_session.lock().await;
        editor.faction_draft.clone()
    };
    let Some(mut draft) = draft else {
        return Ok(Some(ok_response("no active faction draft.".to_string(), None)));
    };

    let seed = generate_faction_seed(
        GenerateFactionSeedInput {
            prompt: draft.seed_prompt.clone(),
        },
        state.clone(),
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
        editor.mode = EditorMode::Faction;
        editor.npc_draft = None;
        editor.location_draft = None;
        editor.faction_draft = Some(draft.clone());
    }

    Ok(Some(ok_response(
        faction_summary_text(&draft),
        Some(faction_event_from_draft(&draft)),
    )))
}

fn cancel_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "cancel",
        metadata_for("cancel"),
        DesktopHandler::new(|invocation| Box::pin(async move { handle_cancel(invocation).await })),
    )
}

async fn handle_cancel(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
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
                ok_response(
                    "npc draft discarded.".to_string(),
                    Some(CommandClientEvent::ClearDrafts),
                )
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
                ok_response(
                    "location draft discarded.".to_string(),
                    Some(CommandClientEvent::ClearDrafts),
                )
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
                ok_response(
                    "faction draft discarded.".to_string(),
                    Some(CommandClientEvent::ClearDrafts),
                )
            }
        }
        EditorMode::None => ok_response("no active draft to cancel.".to_string(), None),
    };

    Ok(Some(response))
}

async fn handle_undo(
    invocation: DesktopHandlerInvocation<'_>,
) -> Result<Option<CommandResponse>, String> {
    let result = undo_last_soft_delete(invocation.state.clone()).await?;
    let output = [
        "## Undo complete".to_string(),
        format!("type: {}", result.entity_type.as_str()),
        format!("name: {}", result.name),
        format!("slug: {}", result.slug),
        format!("vault: {}", path_for_display(&result.vault_path)),
    ]
    .join("\n");
    Ok(Some(ok_response(output, None)))
}

async fn handle_npc(
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

async fn npc_rename(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
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
    Ok(Some(ok_response(
        npc_summary_text(&draft),
        Some(npc_event_from_draft(&draft)),
    )))
}

async fn npc_set(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
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

    Ok(Some(ok_response(
        npc_summary_text(&draft),
        Some(npc_event_from_draft(&draft)),
    )))
}

async fn npc_travel(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    if !trimmed.to_ascii_lowercase().starts_with("npc travel to ") {
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

    Ok(Some(ok_response(
        npc_summary_text(&draft),
        Some(npc_event_from_draft(&draft)),
    )))
}

async fn npc_save(state: State<'_, AppState>) -> Result<Option<CommandResponse>, String> {
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
        editor.faction_draft = None;
    }

    let output = [
        "## NPC saved".to_string(),
        format!("id: {}", result.id),
        format!("slug: {}", result.slug),
        format!("vault: {}", path_for_display(&result.vault_path)),
        format!("updated: {}", result.updated_at),
    ]
    .join("\n");

    Ok(Some(ok_response(
        output,
        Some(CommandClientEvent::ClearDrafts),
    )))
}

async fn npc_reroll(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    if trimmed.eq_ignore_ascii_case("npc reroll") {
        return Ok(Some(ok_response(
            "usage: npc reroll <field> [prompt]".to_string(),
            None,
        )));
    }

    if trimmed.len() <= 11 {
        return Ok(Some(ok_response(
            "usage: npc reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let args = trimmed[11..].trim();
    if args.is_empty() {
        return Ok(Some(ok_response(
            "usage: npc reroll <field> [prompt]".to_string(),
            None,
        )));
    }
    let mut split = args.splitn(2, char::is_whitespace);
    let field = split.next().unwrap_or_default().trim().to_string();
    let prompt = normalize_optional_prompt(split.next().map(|value| value.to_string()));

    let mut draft = {
        let editor = state.editor_session.lock().await;
        editor.npc_draft.clone()
    }
    .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

    let prompt = merge_seed_and_reroll_prompt(&draft.seed_prompt, prompt);

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

    Ok(Some(ok_response(
        npc_summary_text(&draft),
        Some(npc_event_from_draft(&draft)),
    )))
}

async fn create_npc(
    trimmed: &str,
    state: State<'_, AppState>,
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

    let seed = generate_npc_seed(
        GenerateNpcSeedInput {
            prompt: prompt.clone(),
        },
        state.clone(),
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
        editor.mode = EditorMode::Npc;
        editor.location_draft = None;
        editor.npc_draft = Some(draft.clone());
    }

    Ok(Some(ok_response(
        npc_summary_text(&draft),
        Some(npc_event_from_draft(&draft)),
    )))
}

async fn create_location(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
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

    let seed = generate_location_seed(
        GenerateLocationSeedInput {
            prompt: prompt.clone(),
        },
        state.clone(),
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
        editor.mode = EditorMode::Location;
        editor.npc_draft = None;
        editor.location_draft = Some(draft.clone());
    }

    Ok(Some(ok_response(
        location_summary_text(&draft),
        Some(location_event_from_draft(&draft)),
    )))
}

async fn create_faction(
    trimmed: &str,
    state: State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
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

    let seed = generate_faction_seed(
        GenerateFactionSeedInput {
            prompt: prompt.clone(),
        },
        state.clone(),
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
        editor.mode = EditorMode::Faction;
        editor.npc_draft = None;
        editor.location_draft = None;
        editor.faction_draft = Some(draft.clone());
    }

    Ok(Some(ok_response(
        faction_summary_text(&draft),
        Some(faction_event_from_draft(&draft)),
    )))
}

fn normalize_optional_prompt(prompt: Option<String>) -> Option<String> {
    prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn merge_seed_and_reroll_prompt(
    seed_prompt: &Option<String>,
    reroll_prompt: Option<String>,
) -> Option<String> {
    let seed_prompt = seed_prompt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let reroll_prompt = reroll_prompt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    match (seed_prompt, reroll_prompt) {
        (Some(seed), Some(reroll)) => Some(format!(
            "Seed context from original create command:\n{}\n\nReroll request:\n{}",
            seed, reroll
        )),
        (Some(seed), None) => Some(seed.to_string()),
        (None, Some(reroll)) => Some(reroll.to_string()),
        (None, None) => None,
    }
}

async fn build_load_response(
    entity: EntityDetails,
    state: State<'_, AppState>,
) -> (String, Option<CommandClientEvent>) {
    match entity.entity_type {
        EntityType::Npc => {
            let draft = NpcDraftSession {
                id: entity.id.clone(),
                seed_prompt: None,
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
                seed_prompt: None,
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
        EntityType::Faction => {
            let draft = FactionDraftSession {
                id: entity.id.clone(),
                seed_prompt: None,
                name: entity.name.clone(),
                slug: entity.slug.clone(),
                vault_path: path_for_display(&entity.vault_path),
                kind_type: entity
                    .kind_type
                    .clone()
                    .unwrap_or_else(|| "other".to_string()),
                kind_custom: entity.kind_custom.clone(),
                public_description: entity
                    .public_description
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                true_agenda: entity
                    .true_agenda
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                methods: entity.methods.clone().unwrap_or_else(|| "Unknown".to_string()),
                leadership: entity
                    .leadership
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                headquarters: entity
                    .headquarters
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                sphere_of_influence: entity
                    .sphere_of_influence
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                resources_assets: entity
                    .resources_assets
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                allies: entity
                    .allies
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()]),
                rivals_enemies: entity
                    .rivals_enemies
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()]),
                reputation: entity
                    .reputation
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                current_tension: entity
                    .current_tension
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                goals_short_term: entity
                    .goals_short_term
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()]),
                goals_long_term: entity
                    .goals_long_term
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()]),
                symbol_description: entity
                    .symbol_description
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            };
            {
                let mut editor = state.editor_session.lock().await;
                editor.mode = EditorMode::Faction;
                editor.npc_draft = None;
                editor.location_draft = None;
                editor.faction_draft = Some(draft.clone());
            }

            (build_entity_card_text(&entity), Some(faction_event_from_draft(&draft)))
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
        EntityType::Faction => {
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
                "public",
                entity
                    .public_description
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "agenda",
                entity
                    .true_agenda
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "methods",
                entity.methods.clone().unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "leadership",
                entity
                    .leadership
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "headquarters",
                entity
                    .headquarters
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "influence",
                entity
                    .sphere_of_influence
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "resources",
                entity
                    .resources_assets
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row(
                "allies",
                entity
                    .allies
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()])
                    .join(", "),
            ));
            rows.push(entity_row(
                "rivals",
                entity
                    .rivals_enemies
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()])
                    .join(", "),
            ));
            rows.push(entity_row(
                "reputation",
                entity
                    .reputation
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
            rows.push(entity_row(
                "goals_short",
                entity
                    .goals_short_term
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()])
                    .join(", "),
            ));
            rows.push(entity_row(
                "goals_long",
                entity
                    .goals_long_term
                    .clone()
                    .unwrap_or_else(|| vec!["Unknown".to_string()])
                    .join(", "),
            ));
            rows.push(entity_row(
                "symbol",
                entity
                    .symbol_description
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            ));
            rows.push(entity_row("path", path_for_display(&entity.vault_path)));

            OutputDoc {
                blocks: vec![entity_card("Faction", rows)],
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
        EntityType::Faction => {
            let kind_type = entity
                .kind_type
                .clone()
                .unwrap_or_else(|| "other".to_string());
            let kind_custom = entity
                .kind_custom
                .clone()
                .unwrap_or_else(|| "(none)".to_string());
            let public_description = entity
                .public_description
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let true_agenda = entity
                .true_agenda
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let methods = entity.methods.clone().unwrap_or_else(|| "Unknown".to_string());
            let leadership = entity
                .leadership
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let headquarters = entity
                .headquarters
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let sphere_of_influence = entity
                .sphere_of_influence
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let resources_assets = entity
                .resources_assets
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let allies = entity
                .allies
                .clone()
                .unwrap_or_else(|| vec!["Unknown".to_string()])
                .join(", ");
            let rivals = entity
                .rivals_enemies
                .clone()
                .unwrap_or_else(|| vec!["Unknown".to_string()])
                .join(", ");
            let reputation = entity
                .reputation
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let current_tension = entity
                .current_tension
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let goals_short = entity
                .goals_short_term
                .clone()
                .unwrap_or_else(|| vec!["Unknown".to_string()])
                .join(", ");
            let goals_long = entity
                .goals_long_term
                .clone()
                .unwrap_or_else(|| vec!["Unknown".to_string()])
                .join(", ");
            let symbol_description = entity
                .symbol_description
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());

            format!(
                "## Faction\nname: {}\nslug: {}\nkind: {}\nkind_custom: {}\npublic: {}\nagenda: {}\nmethods: {}\nleadership: {}\nheadquarters: {}\ninfluence: {}\nresources: {}\nallies: {}\nrivals: {}\nreputation: {}\ntension: {}\ngoals_short: {}\ngoals_long: {}\nsymbol: {}\npath: {}",
                entity.name,
                entity.slug,
                kind_type,
                kind_custom,
                public_description,
                true_agenda,
                methods,
                leadership,
                headquarters,
                sphere_of_influence,
                resources_assets,
                allies,
                rivals,
                reputation,
                current_tension,
                goals_short,
                goals_long,
                symbol_description,
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

fn faction_event_from_draft(draft: &FactionDraftSession) -> CommandClientEvent {
    CommandClientEvent::LoadFactionDraft {
        id: draft.id.clone(),
        name: draft.name.clone(),
        slug: draft.slug.clone(),
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

fn faction_summary_text(draft: &FactionDraftSession) -> String {
    format!(
        "## Faction Draft\nname: {}\nslug: {}\nkind: {}\nkind_custom: {}\npublic: {}\nagenda: {}\nmethods: {}\nleadership: {}\nheadquarters: {}\ninfluence: {}\nresources: {}\nallies: {}\nrivals: {}\nreputation: {}\ntension: {}\ngoals_short: {}\ngoals_long: {}\nsymbol: {}\npath: {}",
        draft.name,
        draft.slug,
        draft.kind_type,
        draft.kind_custom.as_deref().unwrap_or("(none)"),
        draft.public_description,
        draft.true_agenda,
        draft.methods,
        draft.leadership,
        draft.headquarters,
        draft.sphere_of_influence,
        draft.resources_assets,
        draft.allies.join(", "),
        draft.rivals_enemies.join(", "),
        draft.reputation,
        draft.current_tension,
        draft.goals_short_term.join(", "),
        draft.goals_long_term.join(", "),
        draft.symbol_description,
        draft.vault_path,
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
