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
    let aliased = normalize_alias_tokens_only(tokens, manifest);
    normalize_help_tokens(&aliased)
}

fn normalize_alias_tokens_only(tokens: &[String], manifest: &CommandManifest) -> Vec<String> {
    for alias in &manifest.aliases {
        if tokens.len() >= alias.from.len()
            && tokens
                .iter()
                .take(alias.from.len())
                .zip(alias.from.iter())
                .all(|(left, right)| left.eq_ignore_ascii_case(right))
        {
            let mut rewritten = alias.to.clone();
            rewritten.extend_from_slice(&tokens[alias.from.len()..]);
            return rewritten;
        }
    }

    tokens.to_vec()
}

fn normalize_help_tokens(tokens: &[String]) -> Vec<String> {
    if tokens.len() > 1 && tokens[0].eq_ignore_ascii_case("help") {
        let mut rewritten = tokens[1..].to_vec();
        rewritten.push("help".to_string());
        return rewritten;
    }

    tokens.to_vec()
}

fn expand_inline_delta_root_tokens(tokens: Vec<String>) -> Vec<String> {
    if tokens.is_empty() {
        return tokens;
    }

    let first = &tokens[0];
    if let Some((sign, remainder)) = split_inline_delta_token(first) {
        let mut rewritten = Vec::with_capacity(tokens.len() + 1);
        rewritten.push(sign);
        rewritten.push(remainder);
        rewritten.extend(tokens.into_iter().skip(1));
        rewritten
    } else {
        tokens
    }
}

fn split_inline_delta_token(token: &str) -> Option<(String, String)> {
    if token.len() < 2 {
        return None;
    }

    let mut chars = token.chars();
    let first = chars.next()?;
    if first != '+' && first != '-' {
        return None;
    }

    let remainder: String = chars.collect();
    if remainder.is_empty() {
        return None;
    }

    Some((first.to_string(), remainder))
}

pub fn parse_command_input_with_manifest(input: &str, manifest: &CommandManifest) -> ParseResult {
    let normalized_input = normalize_command_input(input);
    let sanitized_input = sanitize_inline_apostrophes(&normalized_input);
    let ends_with_space = normalized_input
        .chars()
        .last()
        .is_some_and(char::is_whitespace);

    let raw_tokens = match shell_words::split(&sanitized_input) {
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

    let raw_tokens = expand_inline_delta_root_tokens(raw_tokens);

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
        } else if root.requires_subcommand {
            valid = false;
            diagnostics.push(ParseDiagnostic {
                code: "unknown_subcommand".to_string(),
                message: format!("unknown subcommand for {}: {}", root.name, subcommand_name),
            });
            ParseStage::Subcommand
        } else {
            // The root accepts free-form arguments (e.g. `publish <name>`), so an
            // unrecognized second token is an argument, not an unknown subcommand.
            ParseStage::Argument
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

fn sanitize_inline_apostrophes(input: &str) -> String {
    if !input.contains('\'') {
        return input.to_string();
    }

    let mut sanitized = String::with_capacity(input.len());
    let mut prev_char: Option<char> = None;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    for ch in input.chars() {
        match ch {
            '"' => {
                if !in_single_quote {
                    in_double_quote = !in_double_quote;
                }
                sanitized.push(ch);
            }
            '\'' => {
                if in_double_quote {
                    sanitized.push(ch);
                } else if in_single_quote {
                    in_single_quote = false;
                    sanitized.push(ch);
                } else if matches!(prev_char, Some('\\')) {
                    sanitized.push(ch);
                } else if is_quote_start_boundary(prev_char) {
                    in_single_quote = true;
                    sanitized.push(ch);
                } else {
                    sanitized.push('\\');
                    sanitized.push('\'');
                }
            }
            _ => sanitized.push(ch),
        }

        prev_char = Some(ch);
    }

    sanitized
}

fn is_quote_start_boundary(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(ch) => {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '=' | ':'
                        | ','
                        | ';'
                        | '('
                        | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '<'
                        | '>'
                        | '|'
                        | '&'
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_command_input, parse_command_input, ParseStage};

    #[test]
    fn parses_quoted_arguments() {
        let parsed = parse_command_input("config show \"/tmp/with space\"");
        assert!(parsed.valid);
        assert_eq!(parsed.raw_tokens[2], "/tmp/with space");
    }

    #[test]
    fn parses_single_quoted_arguments() {
        let parsed = parse_command_input("config show '/tmp/with space'");
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
    fn normalizes_help_prefix_for_command() {
        let parsed = parse_command_input("help config");
        assert!(parsed.valid);
        assert_eq!(parsed.normalized_tokens, vec!["config", "help"]);
        assert_eq!(parsed.canonical_input, "config help");
    }

    #[test]
    fn normalizes_help_prefix_for_subcommand() {
        let parsed = parse_command_input("help config show");
        assert!(parsed.valid);
        assert_eq!(parsed.normalized_tokens, vec!["config", "show", "help"]);
        assert_eq!(parsed.canonical_input, "config show help");
    }

    #[test]
    fn reports_unknown_subcommand() {
        let parsed = parse_command_input("config nope");
        assert!(!parsed.valid);
        assert_eq!(parsed.diagnostics[0].code, "unknown_subcommand");
    }

    #[test]
    fn treats_free_form_argument_as_argument_not_subcommand() {
        // `publish` declares a `help` subcommand but `requires_subcommand: false`,
        // so an entity name must parse as an argument rather than erroring.
        let parsed = parse_command_input("publish The Brotherhood");
        assert!(
            parsed.valid,
            "expected publish <name> to be valid, diagnostics: {:?}",
            parsed.diagnostics
        );
        assert!(matches!(parsed.completion.stage, ParseStage::Argument));
        assert_eq!(parsed.completion.subcommand, None);
    }

    #[test]
    fn still_matches_known_subcommand_for_free_form_command() {
        let parsed = parse_command_input("publish help");
        assert!(parsed.valid);
        assert_eq!(parsed.completion.subcommand.as_deref(), Some("help"));
    }

    #[test]
    fn parses_setup_vault_command() {
        let parsed = parse_command_input("setup vault");
        assert!(
            parsed.valid,
            "expected setup vault to be valid, diagnostics: {:?}",
            parsed.diagnostics
        );
        assert_eq!(parsed.normalized_tokens, vec!["setup", "vault"]);
    }

    #[test]
    fn parses_setup_llm_command() {
        let parsed = parse_command_input("setup llm");
        assert!(
            parsed.valid,
            "expected setup llm to be valid, diagnostics: {:?}",
            parsed.diagnostics
        );
        assert_eq!(parsed.normalized_tokens, vec!["setup", "llm"]);
    }

    #[test]
    fn parses_setup_model_command() {
        let parsed = parse_command_input("setup model");
        assert!(
            parsed.valid,
            "expected setup model to be valid, diagnostics: {:?}",
            parsed.diagnostics
        );
        assert_eq!(parsed.normalized_tokens, vec!["setup", "model"]);
    }

    #[test]
    fn parses_model_command() {
        let parsed = parse_command_input("model");
        assert!(
            parsed.valid,
            "expected model to be valid, diagnostics: {:?}",
            parsed.diagnostics
        );
        assert_eq!(parsed.normalized_tokens, vec!["model"]);
    }

    #[test]
    fn parses_ping_command() {
        let parsed = parse_command_input("ping");
        assert!(
            parsed.valid,
            "expected ping to be valid, diagnostics: {:?}",
            parsed.diagnostics
        );
        assert_eq!(parsed.normalized_tokens, vec!["ping"]);
    }

    #[test]
    fn reconnect_aliases_to_ping() {
        let parsed = parse_command_input("reconnect");
        assert!(
            parsed.valid,
            "expected reconnect to be valid, diagnostics: {:?}",
            parsed.diagnostics
        );
        assert_eq!(parsed.normalized_tokens, vec!["ping"]);
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

    #[test]
    fn splits_inline_positive_delta_root_token() {
        let parsed = parse_command_input("+1d");
        assert_eq!(parsed.normalized_tokens, vec!["+", "1d"]);
        assert!(parsed.valid);
    }

    #[test]
    fn splits_inline_negative_delta_root_token() {
        let parsed = parse_command_input("-15m");
        assert_eq!(parsed.normalized_tokens, vec!["-", "15m"]);
        assert!(parsed.valid);
    }

    #[test]
    fn parses_start_setup_command() {
        let parsed = parse_command_input("start setup");
        assert!(
            parsed.valid,
            "expected start setup to be valid, diagnostics: {:?}",
            parsed.diagnostics
        );
        assert_eq!(parsed.normalized_tokens, vec!["start", "setup"]);
        assert_eq!(parsed.canonical_input, "start setup");
    }

    #[test]
    fn tokenizes_words_with_inline_apostrophes() {
        let parsed = parse_command_input("create npc mariner's concordance");
        assert!(parsed.valid);
        assert!(parsed.raw_tokens.iter().any(|token| token == "mariner's"));
    }

    #[test]
    fn tokenizes_possessive_suffixes_without_closing_quote() {
        let parsed = parse_command_input("create npc pirates' cove");
        assert!(parsed.valid);
        assert!(parsed.raw_tokens.iter().any(|token| token == "pirates'"));
    }
}
