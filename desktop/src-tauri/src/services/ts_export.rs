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
use crate::boot::{BootPlan, BootTaskInfo, BootTaskResult, BootTone};

const HEADER: &str = "\
// Auto-generated from the Rust types in `command-specs` + `services::suggestions`
// via ts-rs. Do not edit by hand. Regenerate with:
//   UPDATE_MODELS=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml
";

const BOOT_HEADER: &str = "\
// Auto-generated from the Rust types in `crate::boot` via ts-rs. Do not edit by
// hand. Regenerate with:
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

/// Build the full `boot.ts` contents — the boot subsystem types that cross the
/// Tauri boundary. Kept separate from `manifest.ts` so each generated file stays
/// focused on one contract. Dependency-first order (tone/info before the structs
/// that embed them), though TS `type` aliases hoist so it is otherwise cosmetic.
fn generate_boot() -> String {
    let cfg = Config::new();
    let decls = [
        BootTone::decl(&cfg),
        BootTaskInfo::decl(&cfg),
        BootPlan::decl(&cfg),
        BootTaskResult::decl(&cfg),
    ];
    let body = decls
        .iter()
        .map(|decl| format!("export {decl}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{BOOT_HEADER}\n{body}\n")
}

/// Either rewrite `path` with the freshly generated TS (when `UPDATE_MODELS` is
/// set) or assert the on-disk file already matches — the write-or-assert half of
/// every drift guard in this module.
fn assert_or_update(path: &str, generated: &str) {
    if std::env::var("UPDATE_MODELS").is_ok() {
        std::fs::write(path, generated).expect("failed to write generated TS file");
        return;
    }

    let on_disk = std::fs::read_to_string(path).expect("failed to read generated TS file");
    assert_eq!(
        on_disk, generated,
        "{path} is out of sync with the Rust types. \
         Regenerate with `UPDATE_MODELS=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml`."
    );
}

#[test]
fn manifest_ts_matches_the_rust_types() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/generated/manifest.ts");
    assert_or_update(path, &generate());
}

#[test]
fn boot_ts_matches_the_rust_types() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/generated/boot.ts");
    assert_or_update(path, &generate_boot());
}
