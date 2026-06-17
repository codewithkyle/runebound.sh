pub mod calendar_commands;
pub mod create_commands;
pub mod date_commands;
pub mod dungeon_commands;
pub mod entity_commands;
pub mod event_commands;
pub mod faction_commands;
pub mod god_commands;
pub mod item_commands;
pub mod location_commands;
pub mod moon_commands;
pub mod npc_commands;
pub mod publish_commands;
pub mod setup_commands;
pub mod system_commands;
pub mod time_delta_commands;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use command_handler::{HandlerBridge, HandlerEntry, HandlerMetadata, HandlerRegistry};
use runebound_models::output::{
    InlineNode, command_ref, doc, list, paragraph_text, paragraph_with_inlines, text_node,
};
use runebound_models::{
    CommandClientEvent, CommandResponse, OutputDoc, OutputSegment, OutputSegmentKind,
};
use tauri::State;

use crate::app_state::AppState;
use command_specs::handler_metadata_for;

pub type CommandHandlerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<CommandResponse>, String>> + Send + 'a>>;

pub struct DesktopHandler {
    inner:
        Arc<dyn for<'a> Fn(DesktopHandlerInvocation<'a>) -> CommandHandlerFuture<'a> + Send + Sync>,
}

impl DesktopHandler {
    fn new<F>(handler: F) -> Self
    where
        F: for<'a> Fn(DesktopHandlerInvocation<'a>) -> CommandHandlerFuture<'a>
            + Send
            + Sync
            + 'static,
    {
        Self {
            inner: Arc::new(handler),
        }
    }
}

impl HandlerBridge for DesktopHandler {
    type Output = Result<Option<CommandResponse>, String>;
    type Invocation<'a> = DesktopHandlerInvocation<'a>;

    fn invoke<'a>(
        &'a self,
        invocation: Self::Invocation<'a>,
    ) -> command_handler::HandlerFuture<'a, Self::Output> {
        (self.inner)(invocation)
    }
}

pub struct DesktopHandlerInvocation<'a> {
    pub raw_input: &'a str,
    pub tokens: &'a [String],
    pub lowered: &'a [String],
    pub state: State<'a, AppState>,
    pub app_handle: tauri::AppHandle,
}

pub fn desktop_handler_registry() -> &'static HandlerRegistry<DesktopHandler> {
    static REGISTRY: OnceLock<HandlerRegistry<DesktopHandler>> = OnceLock::new();
    REGISTRY.get_or_init(build_desktop_handler_registry)
}

fn build_desktop_handler_registry() -> HandlerRegistry<DesktopHandler> {
    let mut registry = HandlerRegistry::new();
    registry.register(exit_handler_entry());
    registry.register(help_handler_entry());
    registry.register(clear_handler_entry());
    registry.register(history_handler_entry());
    registry.register(create_handler_entry());
    registry.register(npc_handler_entry());
    registry.register(location_handler_entry());
    registry.register(faction_handler_entry());
    registry.register(item_handler_entry());
    registry.register(event_handler_entry());
    registry.register(god_handler_entry());
    registry.register(dungeon_handler_entry());
    registry.register(load_handler_entry());
    registry.register(show_handler_entry());
    registry.register(preview_handler_entry());
    registry.register(delete_handler_entry());
    registry.register(undo_handler_entry());
    registry.register(save_handler_entry());
    registry.register(reroll_handler_entry());
    registry.register(cancel_handler_entry());
    registry.register(calendar_handler_entry());
    registry.register(date_handler_entry());
    registry.register(time_delta_add_handler_entry());
    registry.register(time_delta_subtract_handler_entry());
    registry.register(moon_handler_entry());
    registry.register(publish_handler_entry());
    registry
}

fn metadata_for(name: &str) -> HandlerMetadata {
    handler_metadata_for(name)
        .unwrap_or_else(|| panic!("missing handler metadata for {name}"))
        .into()
}

pub fn ok_response(output: String, client_event: Option<CommandClientEvent>) -> CommandResponse {
    // Every plain-text response still carries a structured doc (a single
    // paragraph) so the frontend renders backend nodes and never parses prose.
    // Responses needing headings, lists, or clickable command_refs build an
    // explicit doc via ok_response_with_doc / command_action_response instead.
    let document = doc().with_block(paragraph_text(output.clone()));
    ok_response_with_doc(output, Some(document), client_event)
}

pub fn ok_response_with_doc(
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
        wizard: None,
    }
}

/// A response whose `output_doc` is a single paragraph `prefix` + clickable
/// `command` + `suffix`. The plain-text fallback embeds the command in backticks,
/// matching the prior wording. Use for actionable guidance (`use \`X help\``,
/// `run \`calendar import\``, …) so the command is clickable.
pub fn command_action_response(prefix: &str, command: &str, suffix: &str) -> CommandResponse {
    let fallback = format!("{prefix}`{command}`{suffix}");
    let document = doc().with_block(paragraph_with_inlines(vec![
        text_node(prefix.to_string()),
        command_ref(command.to_string(), command.to_string()),
        text_node(suffix.to_string()),
    ]));
    ok_response_with_doc(fallback, Some(document), None)
}

pub fn exit_handler_entry() -> HandlerEntry<DesktopHandler> {
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

pub fn help_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "help",
        metadata_for("help"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { system_commands::handle_help(invocation).await })
        }),
    )
}

pub fn clear_handler_entry() -> HandlerEntry<DesktopHandler> {
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

pub fn history_handler_entry() -> HandlerEntry<DesktopHandler> {
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
                            )));
                        }
                    }
                } else {
                    20
                };
                let history = {
                    let service = state.command_service.lock().await;
                    service.session().command_history.clone()
                };
                Ok(Some(ok_response_with_doc(
                    render_history_output(&history, limit),
                    Some(render_history_doc(&history, limit)),
                    None,
                )))
            })
        }),
    )
}

pub fn create_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "create",
        metadata_for("create"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { create_commands::handle_create(invocation).await })
        }),
    )
}

pub fn npc_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "npc",
        metadata_for("npc"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { npc_commands::handle_npc(invocation).await })
        }),
    )
}

pub fn location_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "location",
        metadata_for("location"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { location_commands::handle_location(invocation).await })
        }),
    )
}

pub fn faction_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "faction",
        metadata_for("faction"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { faction_commands::handle_faction(invocation).await })
        }),
    )
}

pub fn item_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "item",
        metadata_for("item"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { item_commands::handle_item(invocation).await })
        }),
    )
}

pub fn event_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "event",
        metadata_for("event"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { event_commands::handle_event(invocation).await })
        }),
    )
}

pub fn god_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "god",
        metadata_for("god"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { god_commands::handle_god(invocation).await })
        }),
    )
}

pub fn dungeon_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "dungeon",
        metadata_for("dungeon"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { dungeon_commands::handle_dungeon(invocation).await })
        }),
    )
}

pub fn publish_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "publish",
        metadata_for("publish"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { publish_commands::handle_publish(invocation).await })
        }),
    )
}

pub fn load_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "load",
        metadata_for("load"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { entity_commands::handle_load(invocation).await })
        }),
    )
}

pub fn show_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "show",
        metadata_for("show"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { entity_commands::handle_show(invocation).await })
        }),
    )
}

pub fn preview_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "preview",
        metadata_for("preview"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { entity_commands::handle_preview(invocation).await })
        }),
    )
}

pub fn delete_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "delete",
        metadata_for("delete"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { entity_commands::handle_delete(invocation).await })
        }),
    )
}

pub fn undo_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "undo",
        metadata_for("undo"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { entity_commands::handle_undo(invocation).await })
        }),
    )
}

pub fn save_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "save",
        metadata_for("save"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { system_commands::handle_save(invocation).await })
        }),
    )
}

pub fn reroll_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "reroll",
        metadata_for("reroll"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { system_commands::handle_reroll(invocation).await })
        }),
    )
}

pub fn cancel_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "cancel",
        metadata_for("cancel"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { system_commands::handle_cancel(invocation).await })
        }),
    )
}

pub fn calendar_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "calendar",
        metadata_for("calendar"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { calendar_commands::handle_calendar(invocation).await })
        }),
    )
}

pub fn date_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "date",
        metadata_for("date"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { date_commands::handle_date(invocation).await })
        }),
    )
}

pub fn time_delta_add_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "+",
        metadata_for("+"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { time_delta_commands::handle_time_delta(invocation).await })
        }),
    )
}

pub fn time_delta_subtract_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "-",
        metadata_for("-"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { time_delta_commands::handle_time_delta(invocation).await })
        }),
    )
}

pub fn moon_handler_entry() -> HandlerEntry<DesktopHandler> {
    HandlerEntry::new(
        "moon",
        metadata_for("moon"),
        DesktopHandler::new(|invocation| {
            Box::pin(async move { moon_commands::handle_moon(invocation).await })
        }),
    )
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

/// The structured form of `render_history_output`: each entry is a clickable
/// `command_ref` so the recorded command re-runs on click — the backend supplies
/// the clickability, the frontend never guesses it from the rendered text.
fn render_history_doc(history: &[String], limit: usize) -> OutputDoc {
    if history.is_empty() {
        return doc().with_block(paragraph_text("(no history)"));
    }
    let safe_limit = limit.clamp(1, 50);
    let start = history.len().saturating_sub(safe_limit);
    let items: Vec<Vec<InlineNode>> = history[start..]
        .iter()
        .enumerate()
        .map(|(index, value)| {
            vec![
                text_node(format!("{}: ", start + index + 1)),
                command_ref(value.clone(), value.clone()),
            ]
        })
        .collect();
    doc().with_block(list(items))
}

pub use crate::entities::domains::{
    dungeon_event_from_draft, dungeon_summary_text, event_event_from_draft, event_summary_text,
    faction_event_from_draft, faction_summary_text, god_event_from_draft, god_summary_text,
    item_event_from_draft, item_summary_text, location_event_from_draft, location_summary_text,
    npc_event_from_draft, npc_summary_text,
};

#[cfg(test)]
mod tests {
    use super::build_desktop_handler_registry;
    use command_specs::{CommandExecution, command_manifest, handler_metadata_for};

    /// Onboarding entry commands launched by the generic wizard route
    /// (`start_wizard` via `onboarding_entry_wizard_id`, *before* registry lookup).
    /// They are marked `Desktop`/`Core` in the manifest but have no registry
    /// handler. See docs/command-contexts.md §4.
    const ONBOARDING_INTERCEPTED: &[&str] = &["start", "model"];

    /// Wizard nav verbs handled by the generic wizard route
    /// (`try_execute_active_wizard`) *before* registry lookup, so they have no
    /// registry handler. `cancel` is excluded: it keeps a real editor handler and
    /// is only additionally intercepted while a wizard is active. See
    /// docs/command-contexts.md §4 (route 4).
    const WIZARD_INTERCEPTED: &[&str] = &["continue", "back"];

    #[test]
    fn every_desktop_command_has_a_registered_handler() {
        let registry = build_desktop_handler_registry();
        for command in command_manifest().commands {
            if !matches!(command.execution, CommandExecution::Desktop) {
                continue;
            }
            if ONBOARDING_INTERCEPTED.contains(&command.name.as_str())
                || WIZARD_INTERCEPTED.contains(&command.name.as_str())
            {
                continue;
            }
            assert!(
                registry.get(&command.name).is_some(),
                "manifest declares desktop command `{}` but no handler is registered in \
                 build_desktop_handler_registry()",
                command.name,
            );
        }
    }

    #[test]
    fn every_registered_handler_maps_to_a_manifest_command() {
        // Catches an orphaned handler registered under a name that no longer
        // exists in the manifest (e.g. after a root rename).
        let registry = build_desktop_handler_registry();
        for entry in registry.iter() {
            assert!(
                handler_metadata_for(entry.name).is_some(),
                "handler `{}` is registered but has no manifest entry",
                entry.name,
            );
        }
    }

    #[test]
    fn desktop_registry_includes_the_core_overrides() {
        // `help` and `exit` are registered in the desktop registry to override
        // their core handlers with desktop-state-aware versions. Losing these
        // would silently fall back to the core behavior.
        let registry = build_desktop_handler_registry();
        assert!(
            registry.get("help").is_some(),
            "missing desktop help override"
        );
        assert!(
            registry.get("exit").is_some(),
            "missing desktop exit override"
        );
    }

    #[test]
    fn ok_response_always_carries_a_structured_doc() {
        // The chokepoint guarantee behind deleting the frontend parser: a plain
        // message still arrives as a structured doc, never raw prose to parse.
        let response = super::ok_response("hello".to_string(), None);
        assert!(
            response.output_doc.is_some(),
            "plain responses must carry a doc so the frontend never parses prose",
        );
    }

    #[test]
    fn history_doc_makes_each_entry_a_clickable_command() {
        use runebound_models::output::{InlineNode, OutputBlock};

        let history = vec!["create npc".to_string(), "calendar".to_string()];
        let document = super::render_history_doc(&history, 20);
        let command_refs = document
            .blocks
            .iter()
            .flat_map(|block| match block {
                OutputBlock::List { items } => items.clone(),
                _ => Vec::new(),
            })
            .flatten()
            .filter_map(|node| match node {
                InlineNode::CommandRef { command, .. } => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(command_refs, vec!["create npc", "calendar"]);
    }

    #[test]
    fn no_active_draft_doc_offers_a_clickable_create() {
        use crate::entities::EntityKind;
        use crate::entities::common::no_active_draft_doc;
        use runebound_models::output::{InlineNode, OutputBlock};

        let document = no_active_draft_doc(EntityKind::Npc);
        let has_create = document.blocks.iter().any(|block| {
            match block {
            OutputBlock::Paragraph { inlines } => inlines.iter().any(|node| {
                matches!(node, InlineNode::CommandRef { command, .. } if command == "create npc")
            }),
            _ => false,
        }
        });
        assert!(
            has_create,
            "no-active-draft doc must offer a clickable create"
        );
    }
}
