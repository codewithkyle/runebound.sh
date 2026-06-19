//! Single source of truth for the Rust↔TS contract.
//!
//! The frontend imports `desktop/src/generated/models.ts`. Rather than
//! hand-transcribe every struct (the old `build.rs`, which silently drifted),
//! we derive the TypeScript straight from the Rust types with `ts-rs` and bundle
//! every frontend-facing type into that one file.
//!
//! This test *is* the generator **and** the drift guard:
//!   - `UPDATE_MODELS=1 cargo test -p runebound-models` rewrites `models.ts`.
//!   - a plain `cargo test` (the normal/CI run) instead asserts the on-disk file
//!     matches what the current Rust types would produce, so any change to a
//!     `#[derive(TS)]` type that isn't regenerated fails the build.
//!
//! Only types that actually cross to the frontend are listed here. The
//! `*Frontmatter` types are disk-serialization formats that never reach the UI,
//! so they are intentionally excluded from the contract.

use ts_rs::{Config, TS};

use runebound_models::drafts::{
    DungeonBeat, DungeonDraft, EventDraft, FactionDraft, GodDraft, ItemDraft, LocationDraft,
    NpcDraft,
};
use runebound_models::events::{
    CommandClientEvent, CommandResponse, OutputSegment, OutputSegmentKind, WizardView,
};
use runebound_models::output::{
    EntityCardRow, InlineNode, OutputBlock, OutputDoc, SpinnerState, StatusTone,
};

const HEADER: &str = "\
// Auto-generated from the Rust types in `runebound-models` via ts-rs.
// Do not edit by hand. Regenerate with:
//   UPDATE_MODELS=1 cargo test -p runebound-models
";

/// Build the full `models.ts` contents. Order is dependency-first for
/// readability; TypeScript `type` aliases are hoisted, so it is otherwise
/// cosmetic.
fn generate() -> String {
    let cfg = Config::new();
    // `ts-rs` `decl()` yields `type X = …;` with no `export`; we prefix it.
    let decls = [
        NpcDraft::decl(&cfg),
        LocationDraft::decl(&cfg),
        FactionDraft::decl(&cfg),
        ItemDraft::decl(&cfg),
        EventDraft::decl(&cfg),
        GodDraft::decl(&cfg),
        DungeonBeat::decl(&cfg),
        DungeonDraft::decl(&cfg),
        EntityCardRow::decl(&cfg),
        InlineNode::decl(&cfg),
        StatusTone::decl(&cfg),
        SpinnerState::decl(&cfg),
        OutputBlock::decl(&cfg),
        OutputDoc::decl(&cfg),
        OutputSegmentKind::decl(&cfg),
        OutputSegment::decl(&cfg),
        CommandClientEvent::decl(&cfg),
        WizardView::decl(&cfg),
        CommandResponse::decl(&cfg),
    ];
    let body = decls
        .iter()
        .map(|decl| format!("export {decl}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{HEADER}\n{body}\n")
}

fn models_ts_path() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../desktop/src/generated/models.ts"
    )
}

#[test]
fn models_ts_matches_the_rust_types() {
    let generated = generate();
    let path = models_ts_path();

    if std::env::var("UPDATE_MODELS").is_ok() {
        std::fs::write(path, &generated).expect("failed to write models.ts");
        return;
    }

    let on_disk = std::fs::read_to_string(path).expect("failed to read models.ts");
    assert_eq!(
        on_disk, generated,
        "desktop/src/generated/models.ts is out of sync with the Rust types. \
         Regenerate with `UPDATE_MODELS=1 cargo test -p runebound-models`."
    );
}
