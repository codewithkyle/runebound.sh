use serde::{Deserialize, Serialize};

use crate::command_manifest::{CommandManifest, CommandSpec, command_manifest};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseResult {
    pub raw_input: String,
    pub raw_tokens: Vec<String>,
    pub normalized_tokens: Vec<String>,
    pub canonical_input: String,
    pub valid: bool,
    pub diagnostics: Vec<ParseDiagnostic>,
    pub completion: CompletionContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionContext {
    pub stage: ParseStage,
    pub root: Option<String>,
    pub subcommand: Option<String>,
    pub current_token: String,
    pub ends_with_space: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseStage {
    Root,
    Subcommand,
    Argument,
}

pub fn parse_command_input(input: &str) -> ParseResult {
    let manifest = command_manifest();
    parse_command_input_with_manifest(input, &manifest)
}

pub fn normalize_command_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('`') && trimmed.ends_with('`') {
        let inner = &trimmed[1..trimmed.len() - 1];
        if !inner.contains('`') {
            return inner.trim().to_string();
        }
    }

    input.to_string()
}

pub fn normalize_alias_tokens(tokens: &[String], manifest: &CommandManifest) -> Vec<String> {
    for alias in &manifest.aliases {
        if tokens.len() == alias.from.len()
            && tokens
                .iter()
                .zip(alias.from.iter())
                .all(|(left, right)| left.eq_ignore_ascii_case(right))
        {
            return alias.to.clone();
        }
    }

    tokens.to_vec()
}

pub fn parse_command_input_with_manifest(input: &str, manifest: &CommandManifest) -> ParseResult {
    let normalized_input = normalize_command_input(input);
    let ends_with_space = normalized_input
        .chars()
        .last()
        .is_some_and(char::is_whitespace);

    let raw_tokens = match shell_words::split(&normalized_input) {
        Ok(tokens) => tokens,
        Err(err) => {
            return ParseResult {
                raw_input: input.to_string(),
                raw_tokens: Vec::new(),
                normalized_tokens: Vec::new(),
                canonical_input: String::new(),
                valid: false,
                diagnostics: vec![ParseDiagnostic {
                    code: "tokenize_error".to_string(),
                    message: format!("invalid command input: {err}"),
                }],
                completion: CompletionContext {
                    stage: ParseStage::Root,
                    root: None,
                    subcommand: None,
                    current_token: String::new(),
                    ends_with_space,
                },
            };
        }
    };

    let normalized_tokens = normalize_alias_tokens(&raw_tokens, manifest);
    let canonical_input = normalized_tokens.join(" ");

    if normalized_tokens.is_empty() {
        return ParseResult {
            raw_input: input.to_string(),
            raw_tokens,
            normalized_tokens,
            canonical_input,
            valid: true,
            diagnostics: Vec::new(),
            completion: CompletionContext {
                stage: ParseStage::Root,
                root: None,
                subcommand: None,
                current_token: String::new(),
                ends_with_space,
            },
        };
    }

    let current_token = if ends_with_space {
        String::new()
    } else {
        normalized_tokens.last().cloned().unwrap_or_default()
    };

    let root_name = normalized_tokens[0].to_lowercase();
    let root = find_command(manifest, &root_name);
    if root.is_none() {
        return ParseResult {
            raw_input: input.to_string(),
            raw_tokens,
            normalized_tokens,
            canonical_input,
            valid: false,
            diagnostics: vec![ParseDiagnostic {
                code: "unknown_command".to_string(),
                message: format!("unknown command: {root_name}"),
            }],
            completion: CompletionContext {
                stage: ParseStage::Root,
                root: None,
                subcommand: None,
                current_token,
                ends_with_space,
            },
        };
    }

    let root = root.expect("checked above");
    let mut diagnostics = Vec::new();
    let mut valid = true;
    let mut matched_subcommand = None;

    let stage = if root.subcommands.is_empty() {
        ParseStage::Argument
    } else if normalized_tokens.len() == 1 {
        if ends_with_space {
            ParseStage::Subcommand
        } else {
            ParseStage::Root
        }
    } else {
        let subcommand_name = normalized_tokens[1].to_lowercase();
        let is_known_subcommand = root
            .subcommands
            .iter()
            .any(|item| item.name.eq_ignore_ascii_case(&subcommand_name));

        if is_known_subcommand {
            matched_subcommand = Some(subcommand_name);
            ParseStage::Argument
        } else if normalized_tokens.len() == 2 && !ends_with_space {
            valid = false;
            diagnostics.push(ParseDiagnostic {
                code: "unknown_subcommand".to_string(),
                message: format!("unknown subcommand for {}: {}", root.name, subcommand_name),
            });
            ParseStage::Subcommand
        } else {
            valid = false;
            diagnostics.push(ParseDiagnostic {
                code: "unknown_subcommand".to_string(),
                message: format!("unknown subcommand for {}: {}", root.name, subcommand_name),
            });
            ParseStage::Subcommand
        }
    };

    ParseResult {
        raw_input: input.to_string(),
        raw_tokens,
        normalized_tokens,
        canonical_input,
        valid,
        diagnostics,
        completion: CompletionContext {
            stage,
            root: Some(root.name.clone()),
            subcommand: matched_subcommand,
            current_token,
            ends_with_space,
        },
    }
}

fn find_command<'a>(manifest: &'a CommandManifest, name: &str) -> Option<&'a CommandSpec> {
    manifest
        .commands
        .iter()
        .find(|cmd| cmd.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::{normalize_command_input, parse_command_input};

    #[test]
    fn parses_quoted_arguments() {
        let parsed = parse_command_input("config show \"/tmp/with space\"");
        assert!(parsed.valid);
        assert_eq!(parsed.raw_tokens[2], "/tmp/with space");
    }

    #[test]
    fn normalizes_aliases() {
        let parsed = parse_command_input("history clear");
        assert!(parsed.valid);
        assert_eq!(parsed.normalized_tokens, vec!["clear", "--history"]);
        assert_eq!(parsed.canonical_input, "clear --history");
    }

    #[test]
    fn reports_unknown_subcommand() {
        let parsed = parse_command_input("config nope");
        assert!(!parsed.valid);
        assert_eq!(parsed.diagnostics[0].code, "unknown_subcommand");
    }

    #[test]
    fn normalizes_markdown_wrapped_help_command() {
        assert_eq!(normalize_command_input("`help`"), "help");
        let parsed = parse_command_input("`help`");
        assert!(parsed.valid);
        assert_eq!(parsed.normalized_tokens, vec!["help"]);
    }

    #[test]
    fn normalizes_markdown_wrapped_multi_token_command() {
        assert_eq!(normalize_command_input("  `config show`  "), "config show");
        let parsed = parse_command_input("  `config show`  ");
        assert!(parsed.valid);
        assert_eq!(parsed.normalized_tokens, vec!["config", "show"]);
    }

    #[test]
    fn does_not_unwrap_malformed_nested_backticks() {
        assert_eq!(normalize_command_input("``help``"), "``help``");
    }
}
