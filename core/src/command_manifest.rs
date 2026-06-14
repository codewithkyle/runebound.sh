use serde::{Deserialize, Serialize};

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
    pub clap_managed: bool,
    pub show_in_autocomplete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubcommandSpec {
    pub name: String,
    pub summary: String,
    pub options: Vec<OptionSpec>,
    pub examples: Vec<String>,
    pub clap_managed: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandExecution {
    Core,
    Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandAlias {
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub summary: String,
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
                clap_managed: true,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "create".to_string(),
                summary: "Create world entities in guided editor flows".to_string(),
                examples: vec![
                    "create npc".to_string(),
                    "create npc an inexperienced town guard trying to make a name for himself"
                        .to_string(),
                    "create help".to_string(),
                ],
                subcommands: vec![
                    SubcommandSpec {
                        name: "npc".to_string(),
                        summary: "Start guided NPC creation".to_string(),
                        options: Vec::new(),
                        examples: vec!["create npc".to_string()],
                        clap_managed: false,
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show create command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["create help".to_string()],
                        clap_managed: false,
                    },
                ],
                options: vec![OptionSpec {
                    name: "--help".to_string(),
                    short: Some("-h".to_string()),
                    takes_value: false,
                    value_hint: None,
                    summary: "Show help".to_string(),
                    completion: CompletionHint::None,
                }],
                requires_subcommand: true,
                canonical_help_command: Some("create help".to_string()),
                execution: CommandExecution::Client,
                clap_managed: false,
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
                        clap_managed: true,
                    },
                    SubcommandSpec {
                        name: "test".to_string(),
                        summary: "Run full config diagnostics".to_string(),
                        options: Vec::new(),
                        examples: vec!["config test".to_string()],
                        clap_managed: true,
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show config command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["config help".to_string()],
                        clap_managed: false,
                    },
                ],
                options: vec![OptionSpec {
                    name: "--help".to_string(),
                    short: Some("-h".to_string()),
                    takes_value: false,
                    value_hint: None,
                    summary: "Show help".to_string(),
                    completion: CompletionHint::None,
                }],
                requires_subcommand: true,
                canonical_help_command: Some("config --help".to_string()),
                execution: CommandExecution::Core,
                clap_managed: true,
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
                clap_managed: false,
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
                    clap_managed: false,
                }],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: None,
                execution: CommandExecution::Client,
                clap_managed: false,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "load".to_string(),
                summary: "Load an NPC or location into editor".to_string(),
                examples: vec!["load Elara Meadowlight".to_string()],
                subcommands: Vec::new(),
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Client,
                clap_managed: false,
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
                        clap_managed: false,
                    },
                    SubcommandSpec {
                        name: "rename".to_string(),
                        summary: "Update NPC name".to_string(),
                        options: Vec::new(),
                        examples: vec!["npc rename Father Elen".to_string()],
                        clap_managed: false,
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
                        clap_managed: false,
                    },
                    SubcommandSpec {
                        name: "travel".to_string(),
                        summary: "Move NPC to a location".to_string(),
                        options: Vec::new(),
                        examples: vec!["npc travel to Waterdeep".to_string()],
                        clap_managed: false,
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
                        clap_managed: false,
                    },
                    SubcommandSpec {
                        name: "save".to_string(),
                        summary: "Save active NPC draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["npc save".to_string()],
                        clap_managed: false,
                    },
                    SubcommandSpec {
                        name: "cancel".to_string(),
                        summary: "Discard active NPC draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["npc cancel".to_string()],
                        clap_managed: false,
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show NPC editor command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["npc help".to_string()],
                        clap_managed: false,
                    },
                ],
                options: vec![OptionSpec {
                    name: "--help".to_string(),
                    short: Some("-h".to_string()),
                    takes_value: false,
                    value_hint: None,
                    summary: "Show help".to_string(),
                    completion: CompletionHint::None,
                }],
                requires_subcommand: true,
                canonical_help_command: Some("npc help".to_string()),
                execution: CommandExecution::Client,
                clap_managed: false,
                show_in_autocomplete: true,
            },
            CommandSpec {
                name: "location".to_string(),
                summary: "Edit active location draft".to_string(),
                examples: vec![
                    "location show".to_string(),
                    "location rename Neverwinter Harbor".to_string(),
                    "location save".to_string(),
                ],
                subcommands: vec![
                    SubcommandSpec {
                        name: "show".to_string(),
                        summary: "Show active location draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["location show".to_string()],
                        clap_managed: false,
                    },
                    SubcommandSpec {
                        name: "rename".to_string(),
                        summary: "Update location name".to_string(),
                        options: Vec::new(),
                        examples: vec!["location rename Neverwinter Harbor".to_string()],
                        clap_managed: false,
                    },
                    SubcommandSpec {
                        name: "save".to_string(),
                        summary: "Save active location draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["location save".to_string()],
                        clap_managed: false,
                    },
                    SubcommandSpec {
                        name: "cancel".to_string(),
                        summary: "Discard active location draft".to_string(),
                        options: Vec::new(),
                        examples: vec!["location cancel".to_string()],
                        clap_managed: false,
                    },
                    SubcommandSpec {
                        name: "help".to_string(),
                        summary: "Show location editor command help".to_string(),
                        options: Vec::new(),
                        examples: vec!["location help".to_string()],
                        clap_managed: false,
                    },
                ],
                options: vec![OptionSpec {
                    name: "--help".to_string(),
                    short: Some("-h".to_string()),
                    takes_value: false,
                    value_hint: None,
                    summary: "Show help".to_string(),
                    completion: CompletionHint::None,
                }],
                requires_subcommand: true,
                canonical_help_command: Some("location help".to_string()),
                execution: CommandExecution::Client,
                clap_managed: false,
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
                execution: CommandExecution::Client,
                clap_managed: false,
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
                execution: CommandExecution::Client,
                clap_managed: false,
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
                execution: CommandExecution::Client,
                clap_managed: false,
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
                    clap_managed: false,
                }],
                options: Vec::new(),
                requires_subcommand: true,
                canonical_help_command: Some("setup help".to_string()),
                execution: CommandExecution::Client,
                clap_managed: false,
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
                    clap_managed: false,
                }],
                options: Vec::new(),
                requires_subcommand: false,
                canonical_help_command: None,
                execution: CommandExecution::Client,
                clap_managed: false,
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
                execution: CommandExecution::Client,
                clap_managed: false,
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
                clap_managed: true,
                show_in_autocomplete: true,
            },
        ],
        aliases: vec![
            CommandAlias {
                from: vec!["create".to_string(), "help".to_string()],
                to: vec!["create".to_string(), "--help".to_string()],
                summary: "create help alias".to_string(),
            },
            CommandAlias {
                from: vec!["history".to_string(), "clear".to_string()],
                to: vec!["clear".to_string(), "--history".to_string()],
                summary: "history clear alias".to_string(),
            },
            CommandAlias {
                from: vec!["npc".to_string(), "help".to_string()],
                to: vec!["npc".to_string(), "--help".to_string()],
                summary: "npc help alias".to_string(),
            },
            CommandAlias {
                from: vec!["location".to_string(), "help".to_string()],
                to: vec!["location".to_string(), "--help".to_string()],
                summary: "location help alias".to_string(),
            },
            CommandAlias {
                from: vec!["config".to_string(), "help".to_string()],
                to: vec!["config".to_string(), "--help".to_string()],
                summary: "config help alias".to_string(),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use clap::CommandFactory;

    use super::command_manifest;
    use crate::command::Cli;

    #[test]
    fn clap_managed_roots_match_manifest() {
        let clap = Cli::command();
        let clap_roots: BTreeSet<String> = clap
            .get_subcommands()
            .map(|cmd| cmd.get_name().to_string())
            .collect();

        let manifest = command_manifest();
        let manifest_roots: BTreeSet<String> = manifest
            .commands
            .iter()
            .filter(|cmd| cmd.clap_managed)
            .map(|cmd| cmd.name.clone())
            .collect();

        assert_eq!(manifest_roots, clap_roots);
    }

    #[test]
    fn clap_managed_subcommands_match_manifest() {
        let clap = Cli::command();
        let config_cmd = clap
            .get_subcommands()
            .find(|sub| sub.get_name() == "config")
            .expect("config command should exist");

        let clap_subcommands: BTreeSet<String> = config_cmd
            .get_subcommands()
            .map(|sub| sub.get_name().to_string())
            .collect();

        let manifest = command_manifest();
        let manifest_config = manifest
            .commands
            .iter()
            .find(|cmd| cmd.name == "config")
            .expect("config command should exist in manifest");
        let manifest_subcommands: BTreeSet<String> = manifest_config
            .subcommands
            .iter()
            .filter(|sub| sub.clap_managed)
            .map(|sub| sub.name.clone())
            .collect();

        assert_eq!(manifest_subcommands, clap_subcommands);
    }
}
