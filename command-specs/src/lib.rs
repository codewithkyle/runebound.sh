use command_handler::{ExecutionTarget as HandlerExecutionTarget, HandlerMetadata};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct HandlerMetadataDescriptor {
    pub name: String,
    pub summary: String,
    pub examples: Vec<String>,
    pub show_in_autocomplete: bool,
    pub requires_subcommand: bool,
    pub canonical_help_command: Option<String>,
    pub execution: CommandExecution,
    pub aliases: Vec<String>,
}

impl From<HandlerMetadataDescriptor> for HandlerMetadata {
    fn from(value: HandlerMetadataDescriptor) -> Self {
        HandlerMetadata {
            summary: value.summary,
            examples: value.examples,
            show_in_autocomplete: value.show_in_autocomplete,
            requires_subcommand: value.requires_subcommand,
            execution: value.execution.into(),
            canonical_help: value.canonical_help_command,
            aliases: value.aliases,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandManifest {
    pub commands: Vec<CommandSpec>,
    pub aliases: Vec<CommandAlias>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSpec {
    pub name: String,
    pub summary: String,
    pub examples: Vec<String>,
    pub subcommands: Vec<SubcommandSpec>,
    pub options: Vec<OptionSpec>,
    pub requires_subcommand: bool,
    pub canonical_help_command: Option<String>,
    pub execution: CommandExecution,
    pub show_in_autocomplete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubcommandSpec {
    pub name: String,
    pub summary: String,
    pub options: Vec<OptionSpec>,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionSpec {
    pub name: String,
    pub short: Option<String>,
    pub takes_value: bool,
    pub value_hint: Option<ValueHint>,
    pub summary: String,
    pub completion: CompletionHint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueHint {
    Path,
    Url,
    Model,
    Integer,
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionHint {
    None,
    StaticChoices(Vec<String>),
    DynamicProvider(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandExecution {
    Core,
    Desktop,
}

impl From<CommandExecution> for HandlerExecutionTarget {
    fn from(value: CommandExecution) -> Self {
        match value {
            CommandExecution::Core => HandlerExecutionTarget::Core,
            CommandExecution::Desktop => HandlerExecutionTarget::Desktop,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandAlias {
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub summary: String,
}

/// Runtime input context that gates which commands are offered in autocomplete.
///
/// Derived from editor state: an open entity draft, the setup/config wizard, or
/// neither (the default command surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputContext {
    Default,
    ConfigEditor,
    /// An entity draft is open; the tag is the entity's command root ("npc", ...).
    EntityEditor(String),
}

/// Declarative visibility of a command across input contexts.
///
/// This is the single source of truth for context-gated autocomplete: the runtime
/// asks [`command_availability`] for a command and never hard-codes per-command
/// visibility rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAvailability {
    /// Default surface only (no editor open): create, calendar, undo, ...
    Default,
    /// Every context (help, clear).
    Always,
    /// Only while the setup/config editor is active.
    ConfigEditor,
    /// Any active editor — config or entity (save, cancel).
    AnyEditor,
    /// Default surface plus any open entity draft (publish).
    DefaultOrEntityEditor,
    /// Only while some entity draft is open (reroll).
    EntityEditorOnly,
    /// Only while the matching entity kind's editor is active (npc, location, ...).
    EntityScoped(&'static str),
}

impl CommandAvailability {
    pub fn is_visible_in(self, context: &InputContext) -> bool {
        match self {
            CommandAvailability::Always => true,
            CommandAvailability::Default => matches!(context, InputContext::Default),
            CommandAvailability::ConfigEditor => matches!(context, InputContext::ConfigEditor),
            CommandAvailability::AnyEditor => matches!(
                context,
                InputContext::ConfigEditor | InputContext::EntityEditor(_)
            ),
            CommandAvailability::DefaultOrEntityEditor => matches!(
                context,
                InputContext::Default | InputContext::EntityEditor(_)
            ),
            CommandAvailability::EntityEditorOnly => {
                matches!(context, InputContext::EntityEditor(_))
            }
            CommandAvailability::EntityScoped(tag) => {
                matches!(context, InputContext::EntityEditor(active) if active == tag)
            }
        }
    }
}

/// Single source of truth for which input context(s) each command is offered in.
///
/// Adding a new entity kind is data-only: add an `EntityScoped` arm here and a
/// schema entry; the suggestion filter itself never changes. Commands not listed
/// default to the default surface.
pub fn command_availability(name: &str) -> CommandAvailability {
    match name {
        "npc" => CommandAvailability::EntityScoped("npc"),
        "location" => CommandAvailability::EntityScoped("location"),
        "faction" => CommandAvailability::EntityScoped("faction"),
        "item" => CommandAvailability::EntityScoped("item"),
        "reroll" => CommandAvailability::EntityEditorOnly,
        "save" | "cancel" => CommandAvailability::AnyEditor,
        "publish" => CommandAvailability::DefaultOrEntityEditor,
        "help" | "clear" => CommandAvailability::Always,
        _ => CommandAvailability::Default,
    }
}

pub fn handler_metadata_for(root: &str) -> Option<HandlerMetadataDescriptor> {
    let manifest = command_manifest();
    let command = manifest
        .commands
        .into_iter()
        .find(|command| command.name.eq_ignore_ascii_case(root))?;

    let aliases = manifest
        .aliases
        .into_iter()
        .filter(|alias| {
            alias
                .from
                .first()
                .map(|token| token.eq_ignore_ascii_case(&command.name))
                .unwrap_or(false)
        })
        .map(|alias| alias.from.join(" "))
        .collect();

    Some(HandlerMetadataDescriptor {
        name: command.name.clone(),
        summary: command.summary,
        examples: command.examples,
        show_in_autocomplete: command.show_in_autocomplete,
        requires_subcommand: command.requires_subcommand,
        canonical_help_command: command.canonical_help_command,
        execution: command.execution,
        aliases,
    })
}

pub fn command_manifest() -> CommandManifest {
    CommandManifest {
        commands: vec![
            CommandSpec {
                name: "status".to_string(),
                summary: "Run readiness checks for configured services".to_string(),
                examples: vec!["status".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Core,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "create".to_string(),
                summary: "Create world entities in guided editor flows".to_string(),
                examples: vec![
                    "create npc".to_string(),
                    "create npc an inexperienced town guard trying to make a name for himself"
                        .to_string(),
                    "create location".to_string(),
                    "create location a swamp trade post controlled by smugglers".to_string(),
                    "create faction a secretive maritime trade cartel".to_string(),
                    "create item a cursed blade woven from stormglass".to_string(),
                    "create help".to_string(),
                ],
                subcommands: vec![
                    SubcommandSpec {
                        name: "npc".to_string(),
                        summary: "Start guided NPC creation".to_string(),
                        options: Vec::new(),
                        examples: vec!["create npc".to_string()],
                    },
                    SubcommandSpec {
                        name: "location".to_string(),
                        summary: "Start guided location creation".to_string(),
                        options: Vec::new(),
                        examples: vec!["create location".to_string()],
                    },
                    SubcommandSpec {
                        name: "faction".to_string(),
                        summary: "Start guided faction creation".to_string(),
                        options: Vec::new(),
                        examples: vec!["create faction".to_string()],
                    },
                    SubcommandSpec {
                        name: "item".to_string(),
                        summary: "Start guided item creation".to_string(),
                        options: Vec::new(),
                        examples: vec!["create item".to_string()],
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show create command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["create help".to_string()],
                    },
                ],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: Some("create help".to_string()),
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "config".to_string(),
                summary: "Inspect and validate configuration".to_string(),
                examples: vec!["config show".to_string(), "config test".to_string()],
                subcommands: vec![
                    SubcommandSpec {
                        name: "show".to_string(),
                        summary: "Display effective config".to_string(),
                        options: Vec::new(),
                        examples: vec!["config show".to_string()],
                    },
                    SubcommandSpec {
                        name: "test".to_string(),
                        summary: "Run full config diagnostics".to_string(),
                        options: Vec::new(),
                        examples: vec!["config test".to_string()],
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show config command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["config help".to_string()],
                    },
                ],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: Some("config help".to_string()),
                execution: CommandExecution::Core,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "help".to_string(),
                summary: "Show top-level help".to_string(),
                examples: vec!["help".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Core,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "start".to_string(),
                summary: "Run interactive desktop flows".to_string(),
                examples: vec!["start setup".to_string()],
                subcommands: vec![SubcommandSpec {
                    name: "setup".to_string(),
                    summary: "Start guided first-time onboarding".to_string(),
                    options: Vec::new(),
                    examples: vec!["start setup".to_string()],
                }],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "load".to_string(),
                summary: "Load an NPC, location, or faction into editor".to_string(),
                examples: vec!["load Elara Meadowlight".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "show".to_string(),
                summary: "Preview an NPC, location, or faction without entering editor".to_string(),
                examples: vec!["show Elara Meadowlight".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "preview".to_string(),
                summary: "Alias for show; previews entity card only".to_string(),
                examples: vec!["preview Neverwinter Harbor".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "npc".to_string(),
                summary: "Edit active NPC draft".to_string(),
                examples: vec![
                    "npc show".to_string(),
                    "npc rename Father Elen".to_string(),
                    "npc travel to Waterdeep".to_string(),
                    "npc save".to_string(),
                ],
                subcommands: vec![
                    SubcommandSpec {
                        name: "show".to_string(),
                        summary: "Show active NPC draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["npc show".to_string()],
                    },
                    SubcommandSpec {
                        name: "rename".to_string(),
                        summary: "Update NPC name".to_string(),
                        options: Vec::new(),
                        examples: vec!["npc rename Father Elen".to_string()],
                    },
                    SubcommandSpec {
                        name: "set".to_string(),
                        summary: "Update NPC fields (except location)".to_string(),
                        options: Vec::new(),
                        examples: vec![
                            "npc set name Father Elen".to_string(),
                            "npc set race Human".to_string(),
                            "npc set occupation Town Guard".to_string(),
                            "npc set sex female".to_string(),
                            "npc set age 42".to_string(),
                            "npc set height 5'11\"".to_string(),
                            "npc set weight 185".to_string(),
                            "npc set background Former caravan guard turned innkeeper".to_string(),
                            "npc set want Secure enough coin to leave town".to_string(),
                            "npc set secret Owes a smuggler blood debt".to_string(),
                            "npc set carrying keys, ledger, hidden dagger".to_string(),
                        ],
                    },
                    SubcommandSpec {
                        name: "travel".to_string(),
                        summary: "Move NPC to a location".to_string(),
                        options: Vec::new(),
                        examples: vec!["npc travel to Waterdeep".to_string()],
                    },
                    SubcommandSpec {
                        name: "reroll".to_string(),
                        summary: "Reroll one NPC field with optional prompt".to_string(),
                        options: Vec::new(),
                        examples: vec![
                            "npc reroll name".to_string(),
                            "npc reroll occupation".to_string(),
                            "npc reroll name a rough and tough hill troll".to_string(),
                            "npc reroll background distrustful of nobles".to_string(),
                        ],
                    },
                    SubcommandSpec {
                        name: "save".to_string(),
                        summary: "Save active NPC draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["npc save".to_string()],
                    },
                    SubcommandSpec {
                        name: "cancel".to_string(),
                        summary: "Discard active NPC draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["npc cancel".to_string()],
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show NPC editor command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["npc help".to_string()],
                    },
                ],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: Some("npc help".to_string()),
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "location".to_string(),
                summary: "Edit active location draft".to_string(),
                examples: vec![
                    "location show".to_string(),
                    "location rename Neverwinter Harbor".to_string(),
                    "location set kind town".to_string(),
                    "location reroll exports".to_string(),
                    "location save".to_string(),
                ],
                subcommands: vec![
                    SubcommandSpec {
                        name: "show".to_string(),
                        summary: "Show active location draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["location show".to_string()],
                    },
                    SubcommandSpec {
                        name: "rename".to_string(),
                        summary: "Update location name".to_string(),
                        options: Vec::new(),
                        examples: vec!["location rename Neverwinter Harbor".to_string()],
                    },
                    SubcommandSpec {
                        name: "set".to_string(),
                        summary: "Update location fields".to_string(),
                        options: Vec::new(),
                        examples: vec![
                            "location set kind city".to_string(),
                            "location set kind_custom drifting armada".to_string(),
                            "location set visual Lantern-lit markets line flooded alleys."
                                .to_string(),
                            "location set history Built on drowned ruins reclaimed by merchants."
                                .to_string(),
                            "location set exports smoked eel, river pearls".to_string(),
                            "location set tone damp suspicious crowded".to_string(),
                            "location set authority Merchants' Compact".to_string(),
                            "location set danger risky".to_string(),
                            "location set tension Guild war brews beneath trade talks.".to_string(),
                        ],
                    },
                    SubcommandSpec {
                        name: "reroll".to_string(),
                        summary: "Reroll one location field with optional prompt".to_string(),
                        options: Vec::new(),
                        examples: vec![
                            "location reroll visual".to_string(),
                            "location reroll history old dwarven colony".to_string(),
                            "location reroll exports".to_string(),
                        ],
                    },
                    SubcommandSpec {
                        name: "save".to_string(),
                        summary: "Save active location draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["location save".to_string()],
                    },
                    SubcommandSpec {
                        name: "cancel".to_string(),
                        summary: "Discard active location draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["location cancel".to_string()],
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show location editor command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["location help".to_string()],
                    },
                ],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: Some("location help".to_string()),
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "faction".to_string(),
                summary: "Edit active faction draft".to_string(),
                examples: vec![
                    "faction show".to_string(),
                    "faction rename Crimson Lantern Syndicate".to_string(),
                    "faction set agenda Control every harbor tax office.".to_string(),
                    "faction reroll symbol".to_string(),
                    "faction save".to_string(),
                ],
                subcommands: vec![
                    SubcommandSpec {
                        name: "show".to_string(),
                        summary: "Show active faction draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["faction show".to_string()],
                    },
                    SubcommandSpec {
                        name: "rename".to_string(),
                        summary: "Update faction name".to_string(),
                        options: Vec::new(),
                        examples: vec!["faction rename Crimson Lantern Syndicate".to_string()],
                    },
                    SubcommandSpec {
                        name: "set".to_string(),
                        summary: "Update faction fields".to_string(),
                        options: Vec::new(),
                        examples: vec![
                            "faction set kind guild".to_string(),
                            "faction set public Smugglers posing as licensed traders.".to_string(),
                            "faction set agenda Control every harbor tax office.".to_string(),
                            "faction set goals_short bribe customs, sabotage rivals".to_string(),
                            "faction set symbol A crimson lantern on black sails.".to_string(),
                        ],
                    },
                    SubcommandSpec {
                        name: "reroll".to_string(),
                        summary: "Reroll one faction field with optional prompt".to_string(),
                        options: Vec::new(),
                        examples: vec![
                            "faction reroll methods".to_string(),
                            "faction reroll goals_long seize inland trade routes".to_string(),
                            "faction reroll symbol nautical but menacing".to_string(),
                        ],
                    },
                    SubcommandSpec {
                        name: "save".to_string(),
                        summary: "Save active faction draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["faction save".to_string()],
                    },
                    SubcommandSpec {
                        name: "cancel".to_string(),
                        summary: "Discard active faction draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["faction cancel".to_string()],
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show faction editor command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["faction help".to_string()],
                    },
                ],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: Some("faction help".to_string()),
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "item".to_string(),
                summary: "Edit active item draft".to_string(),
                examples: vec![
                    "item show".to_string(),
                    "item rename Everember Blade".to_string(),
                    "item set category weapon".to_string(),
                    "item reroll abilities".to_string(),
                    "item save".to_string(),
                ],
                subcommands: vec![
                    SubcommandSpec {
                        name: "show".to_string(),
                        summary: "Show active item draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["item show".to_string()],
                    },
                    SubcommandSpec {
                        name: "rename".to_string(),
                        summary: "Update item name".to_string(),
                        options: Vec::new(),
                        examples: vec!["item rename Everember Blade".to_string()],
                    },
                    SubcommandSpec {
                        name: "set".to_string(),
                        summary: "Update item fields".to_string(),
                        options: Vec::new(),
                        examples: vec![
                            "item set category weapon".to_string(),
                            "item set rarity legendary".to_string(),
                            "item set abilities Channels stormlight into blinding arcs."
                                .to_string(),
                            "item set materials stormglass, quicksilver filigree".to_string(),
                        ],
                    },
                    SubcommandSpec {
                        name: "reroll".to_string(),
                        summary: "Reroll one item field with optional prompt".to_string(),
                        options: Vec::new(),
                        examples: vec![
                            "item reroll name".to_string(),
                            "item reroll abilities lightning themed".to_string(),
                            "item reroll history ancient elven relic".to_string(),
                        ],
                    },
                    SubcommandSpec {
                        name: "save".to_string(),
                        summary: "Save active item draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["item save".to_string()],
                    },
                    SubcommandSpec {
                        name: "cancel".to_string(),
                        summary: "Discard active item draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["item cancel".to_string()],
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show item editor command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["item help".to_string()],
                    },
                ],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: Some("item help".to_string()),
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "save".to_string(),
                summary: "Save active guided flow context".to_string(),
                examples: vec!["save".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "reroll".to_string(),
                summary: "Regenerate content in active editor flow".to_string(),
                examples: vec!["reroll".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "cancel".to_string(),
                summary: "Cancel active editor flow without saving".to_string(),
                examples: vec!["cancel".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "delete".to_string(),
                summary: "Soft delete an NPC, location, or faction".to_string(),
                examples: vec!["delete Elara Meadowlight".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "undo".to_string(),
                summary: "Restore the most recently soft-deleted entity".to_string(),
                examples: vec!["undo".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "setup".to_string(),
                summary: "Guided setup helper commands".to_string(),
                examples: vec!["setup help".to_string()],
                subcommands: vec![
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show guided setup command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["setup help".to_string()],
                    },
                    SubcommandSpec {
                        name: "vault".to_string(),
                        summary: "Change which Obsidian vault is used".to_string(),
                        options: Vec::new(),
                        examples: vec!["setup vault".to_string()],
                    },
                    SubcommandSpec {
                        name: "llm".to_string(),
                        summary: "Change the LLM host/port and model".to_string(),
                        options: Vec::new(),
                        examples: vec!["setup llm".to_string()],
                    },
                    SubcommandSpec {
                        name: "model".to_string(),
                        summary: "Switch the Ollama model on the current server".to_string(),
                        options: Vec::new(),
                        examples: vec!["setup model".to_string()],
                    },
                ],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: Some("setup help".to_string()),
                execution: CommandExecution::Core,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "history".to_string(),
                summary: "Inspect command history in desktop UI".to_string(),
                examples: vec![
                    "history".to_string(),
                    "history 10".to_string(),
                    "history clear".to_string(),
                ],
                subcommands: vec![SubcommandSpec {
                    name: "clear".to_string(),
                    summary: "Clear command history".to_string(),
                    options: Vec::new(),
                    examples: vec!["history clear".to_string()],
                }],
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "clear".to_string(),
                summary: "Clear terminal output in desktop UI".to_string(),
                examples: vec!["clear".to_string(), "clear --history".to_string()],
                subcommands: Vec::new(),
                options: vec![OptionSpec {
                    name: "--history".to_string(),
                    short: None,
                    takes_value: false,
                    value_hint: None,
                    summary: "Also clear command history".to_string(),
                    completion: CompletionHint::None,
                }],
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "calendar".to_string(),
                summary: "Import and manage fantasy calendar".to_string(),
                examples: vec![
                    "calendar import".to_string(),
                    "calendar import path/to/calendar.json".to_string(),
                ],
                subcommands: vec![
                    SubcommandSpec {
                        name: "import".to_string(),
                        summary: "Import calendar from JSON file exported by donjon.bin.sh"
                            .to_string(),
                        options: Vec::new(),
                        examples: vec![
                            "calendar import".to_string(),
                            "calendar import path/to/calendar.json".to_string(),
                        ],
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show calendar command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["calendar help".to_string()],
                    },
                ],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: Some("calendar help".to_string()),
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "date".to_string(),
                summary: "Inspect and modify the current calendar date".to_string(),
                examples: vec![
                    "date".to_string(),
                    "date set year 5".to_string(),
                    "date set month Emberwane".to_string(),
                    "date set day 14".to_string(),
                ],
                subcommands: vec![
                    SubcommandSpec {
                        name: "set".to_string(),
                        summary: "Set a component of the current date".to_string(),
                        options: Vec::new(),
                        examples: vec![
                            "date set year 5".to_string(),
                            "date set month Emberwane".to_string(),
                            "date set day 14".to_string(),
                        ],
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show date command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["date help".to_string()],
                    },
                ],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: Some("date help".to_string()),
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "+".to_string(),
                summary: "Advance the calendar forward by a time delta".to_string(),
                examples: vec!["+15m".to_string(), "+2h".to_string(), "+1d".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                // Hidden from prefix-based autocomplete: users type a delta (e.g. `+15m`),
                // never `+` as a standalone prefix, so it would never surface usefully.
                show_in_autocomplete: false,
            },
            CommandSpec {
                name: "-".to_string(),
                summary: "Rewind the calendar backward by a time delta".to_string(),
                examples: vec!["-10m".to_string(), "-3d".to_string(), "-1w".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                // Hidden from prefix-based autocomplete: users type a delta (e.g. `-3d`),
                // never `-` as a standalone prefix, so it would never surface usefully.
                show_in_autocomplete: false,
            },
            CommandSpec {
                name: "moon".to_string(),
                summary: "Show the current moon phases".to_string(),
                examples: vec!["moon".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "publish".to_string(),
                summary: "Render an entity's markdown into the Obsidian vault".to_string(),
                examples: vec![
                    "publish Lirael".to_string(),
                    "publish obsidian-gate".to_string(),
                ],
                subcommands: vec![SubcommandSpec {
                    name: "help".to_string(),
                    summary: "Show publish command help".to_string(),
                    options: Vec::new(),
                    examples: vec!["publish help".to_string()],
                }],
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: Some("publish help".to_string()),
                execution: CommandExecution::Desktop,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "exit".to_string(),
                summary: "Exit the application".to_string(),
                examples: vec!["exit".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Core,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "model".to_string(),
                summary: "Switch the Ollama model on the current server".to_string(),
                examples: vec!["model".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Core,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "ping".to_string(),
                summary: "Test whether the Ollama LLM server is running".to_string(),
                examples: vec!["ping".to_string(), "reconnect".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Core,
                show_in_autocomplete: true,
            },
        ],
        aliases: vec![
            CommandAlias {
                from: vec!["history".to_string(), "clear".to_string()],
                to: vec!["clear".to_string(), "--history".to_string()],
                summary: "history clear alias".to_string(),
            },
            CommandAlias {
                from: vec!["reconnect".to_string()],
                to: vec!["ping".to_string()],
                summary: "reconnect alias for ping".to_string(),
            },
            CommandAlias {
                from: vec!["setup".to_string(), "start".to_string()],
                to: vec!["start".to_string(), "setup".to_string()],
                summary: "setup start alias".to_string(),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommandAvailability, CommandExecution, InputContext, command_availability, command_manifest,
    };
    use std::collections::HashSet;

    fn manifest_roots() -> Vec<String> {
        command_manifest()
            .commands
            .into_iter()
            .map(|command| command.name)
            .collect()
    }

    #[test]
    fn core_commands_include_expected_roots() {
        let manifest = command_manifest();
        let core_roots: Vec<String> = manifest
            .commands
            .iter()
            .filter(|command| matches!(command.execution, CommandExecution::Core))
            .map(|command| command.name.clone())
            .collect();

        assert!(core_roots.contains(&"status".to_string()));
        assert!(core_roots.contains(&"config".to_string()));
        assert!(core_roots.contains(&"help".to_string()));
        assert!(core_roots.contains(&"exit".to_string()));
    }

    #[test]
    fn desktop_commands_are_not_core() {
        let manifest = command_manifest();
        for command in manifest.commands {
            if matches!(command.execution, CommandExecution::Desktop) {
                assert!(!matches!(command.execution, CommandExecution::Core));
            }
        }
    }

    #[test]
    fn canonical_help_commands_are_phrase_based() {
        let manifest = command_manifest();
        for command in manifest.commands {
            if let Some(help) = command.canonical_help_command {
                assert!(!help.contains("--help"));
                assert!(!help.contains("-h"));
                assert!(help.ends_with(" help") || help == "help");
            }
        }
    }

    // ----------------------------------------------------------------------
    // command_availability / is_visible_in — the single source of truth for
    // context-gated help + autocomplete. docs/command-contexts.md blames a
    // wrong/missing arm here for several 0.4.0 regressions (undo dropped from
    // help, publish behaving inconsistently). These lock the contract.
    // ----------------------------------------------------------------------

    fn npc_editor() -> InputContext {
        InputContext::EntityEditor("npc".to_string())
    }

    fn location_editor() -> InputContext {
        InputContext::EntityEditor("location".to_string())
    }

    #[test]
    fn entity_roots_are_scoped_to_their_own_editor() {
        // Regression guard: these must NOT silently fall through to `Default`.
        for root in ["npc", "location", "faction", "item"] {
            assert_eq!(
                command_availability(root),
                CommandAvailability::EntityScoped(root),
                "{root} should be EntityScoped to its own editor",
            );
        }

        // npc is visible only in the npc editor, nowhere else.
        assert!(command_availability("npc").is_visible_in(&npc_editor()));
        assert!(!command_availability("npc").is_visible_in(&location_editor()));
        assert!(!command_availability("npc").is_visible_in(&InputContext::Default));
        assert!(!command_availability("npc").is_visible_in(&InputContext::ConfigEditor));
    }

    #[test]
    fn reroll_is_visible_only_inside_an_entity_editor() {
        assert_eq!(
            command_availability("reroll"),
            CommandAvailability::EntityEditorOnly
        );
        assert!(command_availability("reroll").is_visible_in(&npc_editor()));
        assert!(command_availability("reroll").is_visible_in(&location_editor()));
        assert!(!command_availability("reroll").is_visible_in(&InputContext::Default));
        assert!(!command_availability("reroll").is_visible_in(&InputContext::ConfigEditor));
    }

    #[test]
    fn save_and_cancel_are_visible_in_any_editor_but_not_default() {
        for root in ["save", "cancel"] {
            assert_eq!(command_availability(root), CommandAvailability::AnyEditor);
            assert!(command_availability(root).is_visible_in(&npc_editor()));
            assert!(command_availability(root).is_visible_in(&InputContext::ConfigEditor));
            assert!(!command_availability(root).is_visible_in(&InputContext::Default));
        }
    }

    #[test]
    fn publish_is_visible_on_default_surface_and_entity_editors() {
        assert_eq!(
            command_availability("publish"),
            CommandAvailability::DefaultOrEntityEditor
        );
        assert!(command_availability("publish").is_visible_in(&InputContext::Default));
        assert!(command_availability("publish").is_visible_in(&npc_editor()));
        // Not the setup wizard.
        assert!(!command_availability("publish").is_visible_in(&InputContext::ConfigEditor));
    }

    #[test]
    fn help_and_clear_are_always_visible() {
        for root in ["help", "clear"] {
            assert_eq!(command_availability(root), CommandAvailability::Always);
            for context in [
                InputContext::Default,
                InputContext::ConfigEditor,
                npc_editor(),
            ] {
                assert!(command_availability(root).is_visible_in(&context));
            }
        }
    }

    #[test]
    fn entity_scoped_visibility_does_not_leak_across_editors() {
        // A footgun if EntityScoped matched any editor: npc commands must stay
        // hidden while a location draft is open, and vice versa.
        assert!(!command_availability("location").is_visible_in(&npc_editor()));
        assert!(!command_availability("npc").is_visible_in(&location_editor()));
    }

    /// Sentinel: the exact set of commands that resolve to the `Default`
    /// availability. The `_ => Default` fallthrough in `command_availability`
    /// is a documented footgun — a new command added without an explicit arm
    /// silently lands here and becomes invisible in every editor context.
    ///
    /// If this test fails because you added a command, do not just add it to
    /// this list: first decide whether `Default`-only is actually correct, and
    /// if not, add an explicit `command_availability` arm.
    #[test]
    fn default_surface_commands_are_an_explicit_known_set() {
        let expected_default: HashSet<&str> = [
            "status", "create", "config", "start", "load", "show", "preview", "delete", "undo",
            "setup", "calendar", "date", "history", "+", "-", "moon", "exit", "model", "ping",
        ]
        .into_iter()
        .collect();

        let actual_default: HashSet<String> = manifest_roots()
            .into_iter()
            .filter(|name| command_availability(name) == CommandAvailability::Default)
            .collect();

        let actual_ref: HashSet<&str> = actual_default.iter().map(String::as_str).collect();
        assert_eq!(
            actual_ref, expected_default,
            "the set of Default-only commands changed; categorize new commands \
             with an explicit command_availability arm (see docs/command-contexts.md)",
        );
    }

    // ----------------------------------------------------------------------
    // requires_subcommand — argument-vs-subcommand parser decision. Setting
    // this wrong on a free-form-argument command was the `publish The
    // Brotherhood` regression (docs/command-contexts.md §3).
    // ----------------------------------------------------------------------

    fn requires_subcommand_for(root: &str) -> bool {
        command_manifest()
            .commands
            .into_iter()
            .find(|command| command.name == root)
            .unwrap_or_else(|| panic!("command {root} missing from manifest"))
            .requires_subcommand
    }

    #[test]
    fn free_form_argument_roots_do_not_require_a_subcommand() {
        // Each of these accepts a free-form name/value as its second token; if
        // requires_subcommand were true the argument would be rejected as an
        // unknown subcommand.
        for root in ["load", "show", "preview", "delete", "history", "publish"] {
            assert!(
                !requires_subcommand_for(root),
                "{root} takes a free-form argument and must be requires_subcommand: false",
            );
        }
    }

    #[test]
    fn menu_style_roots_require_a_subcommand() {
        for root in [
            "create", "config", "npc", "location", "faction", "item", "setup", "calendar", "date",
            "start",
        ] {
            assert!(
                requires_subcommand_for(root),
                "{root} is a menu-style root and must be requires_subcommand: true",
            );
        }
    }

    // ----------------------------------------------------------------------
    // Manifest structural integrity.
    // ----------------------------------------------------------------------

    #[test]
    fn command_roots_are_unique() {
        let roots = manifest_roots();
        let unique: HashSet<&String> = roots.iter().collect();
        assert_eq!(unique.len(), roots.len(), "duplicate command root in manifest");
    }

    #[test]
    fn commands_have_name_summary_and_examples() {
        for command in command_manifest().commands {
            assert!(!command.name.is_empty(), "command has empty name");
            assert!(
                !command.summary.is_empty(),
                "{} has empty summary",
                command.name
            );
            assert!(
                !command.examples.is_empty(),
                "{} has no examples",
                command.name
            );
        }
    }

    #[test]
    fn canonical_help_command_targets_its_own_root() {
        for command in command_manifest().commands {
            if let Some(help) = &command.canonical_help_command {
                let first = help.split_whitespace().next().unwrap_or_default();
                assert_eq!(
                    first, command.name,
                    "{}'s canonical_help_command should start with its own root",
                    command.name
                );
            }
        }
    }

    #[test]
    fn menu_style_roots_declare_subcommands() {
        for command in command_manifest().commands {
            if command.requires_subcommand {
                assert!(
                    !command.subcommands.is_empty(),
                    "{} requires a subcommand but declares none",
                    command.name
                );
            }
        }
    }

    #[test]
    fn alias_targets_resolve_to_real_roots() {
        let roots: HashSet<String> = manifest_roots().into_iter().collect();
        for alias in command_manifest().aliases {
            let target = alias
                .to
                .first()
                .expect("alias must have a target")
                .clone();
            assert!(
                roots.contains(&target),
                "alias {:?} targets unknown root {target}",
                alias.from
            );
        }
    }

    /// Only the hidden delta roots (`+`, `-`) are excluded from autocomplete.
    /// A new command accidentally hidden would silently vanish from typeahead.
    #[test]
    fn only_delta_roots_are_hidden_from_autocomplete() {
        let hidden: HashSet<String> = command_manifest()
            .commands
            .into_iter()
            .filter(|command| !command.show_in_autocomplete)
            .map(|command| command.name)
            .collect();
        let expected: HashSet<String> = ["+", "-"].into_iter().map(String::from).collect();
        assert_eq!(hidden, expected, "unexpected change to autocomplete-hidden roots");
    }
}
