//! Single source of truth for the command-manifest + suggestion Rust↔TS contract.
//!
//! The frontend imports `desktop/src/generated/manifest.ts`. Rather than
//! hand-maintain those types in `parser-client.ts`, we derive the TypeScript
//! straight from the Rust types (`command-specs` manifest types + this crate's
//! suggestion types) with `ts-rs` and bundle them into that one file. Mirrors the
//! `runebound-models` `models.ts` setup.
//!
//! This `#[cfg(test)]` module is both the generator and the drift guard (the
//! desktop crate is a binary with no lib target, so an integration test can't
//! reach these internals — it lives inline instead):
//!   - `UPDATE_MODELS=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml`
//!     rewrites `manifest.ts`.
//!   - a plain `cargo test` asserts the on-disk file matches the Rust types.

use command_specs::{
    CommandAlias, CommandExecution, CommandManifest, CommandSpec, CompletionHint, OptionSpec,
    SpinnerHint, SubcommandSpec, ValueHint,
};
use ts_rs::{Config, TS};

use super::suggestions::{CommandSuggestion, SuggestionHelperText};

const HEADER: &str = "\
// Auto-generated from the Rust types in `command-specs` + `services::suggestions`
// via ts-rs. Do not edit by hand. Regenerate with:
//   UPDATE_MODELS=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml
";

/// Build the full `manifest.ts` contents. Dependency-first order for readability;
/// TypeScript `type` aliases are hoisted, so it is otherwise cosmetic.
fn generate() -> String {
    let cfg = Config::new();
    let decls = [
        ValueHint::decl(&cfg),
        CompletionHint::decl(&cfg),
        CommandExecution::decl(&cfg),
        OptionSpec::decl(&cfg),
        SubcommandSpec::decl(&cfg),
        CommandSpec::decl(&cfg),
        SpinnerHint::decl(&cfg),
        CommandAlias::decl(&cfg),
        CommandManifest::decl(&cfg),
        SuggestionHelperText::decl(&cfg),
        CommandSuggestion::decl(&cfg),
    ];
    let body = decls
        .iter()
        .map(|decl| format!("export {decl}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{HEADER}\n{body}\n")
}

#[test]
fn manifest_ts_matches_the_rust_types() {
    let generated = generate();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/generated/manifest.ts");

    if std::env::var("UPDATE_MODELS").is_ok() {
        std::fs::write(path, &generated).expect("failed to write manifest.ts");
        return;
    }

    let on_disk = std::fs::read_to_string(path).expect("failed to read manifest.ts");
    assert_eq!(
        on_disk, generated,
        "desktop/src/generated/manifest.ts is out of sync with the Rust types. \
         Regenerate with `UPDATE_MODELS=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml`."
    );
}
