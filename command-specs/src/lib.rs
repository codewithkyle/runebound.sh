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
                            "item set abilities Channels stormlight into blinding arcs.".to_string(),
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
                subcommands: vec![SubcommandSpec {
                    name: "help".to_string(),
                    summary: "Show guided setup command help".to_string(),
                    options: Vec::new(),
                    examples: vec!["setup help".to_string()],
                }],
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
                        summary: "Import calendar from JSON file exported by donjon.bin.sh".to_string(),
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
        ],
        aliases: vec![
            CommandAlias {
                from: vec!["history".to_string(), "clear".to_string()],
                to: vec!["clear".to_string(), "--history".to_string()],
                summary: "history clear alias".to_string(),
            },
            CommandAlias {
                from: vec!["start".to_string(), "setup".to_string()],
                to: vec!["setup".to_string(), "start".to_string()],
                summary: "setup start alias".to_string(),
            },
            CommandAlias {
                from: vec!["show".to_string(), "setup".to_string()],
                to: vec!["setup".to_string(), "show".to_string()],
                summary: "setup show alias".to_string(),
            },
            CommandAlias {
                from: vec!["cancel".to_string(), "setup".to_string()],
                to: vec!["setup".to_string(), "cancel".to_string()],
                summary: "setup cancel alias".to_string(),
            },
            CommandAlias {
                from: vec!["set".to_string(), "vault".to_string()],
                to: vec!["setup".to_string(), "set".to_string(), "vault".to_string()],
                summary: "setup set vault alias".to_string(),
            },
            CommandAlias {
                from: vec!["set".to_string(), "ollama".to_string()],
                to: vec!["setup".to_string(), "set".to_string(), "ollama".to_string()],
                summary: "setup set ollama alias".to_string(),
            },
            CommandAlias {
                from: vec!["test".to_string(), "ollama".to_string()],
                to: vec![
                    "setup".to_string(),
                    "test".to_string(),
                    "ollama".to_string(),
                ],
                summary: "setup test ollama alias".to_string(),
            },
            CommandAlias {
                from: vec!["use".to_string(), "model".to_string()],
                to: vec!["setup".to_string(), "use".to_string(), "model".to_string()],
                summary: "setup use model alias".to_string(),
            },
            CommandAlias {
                from: vec!["set".to_string(), "model".to_string()],
                to: vec!["setup".to_string(), "set".to_string(), "model".to_string()],
                summary: "setup set model alias".to_string(),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandExecution, command_manifest};

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
}
