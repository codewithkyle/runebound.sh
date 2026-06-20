mod dungeon;
mod engine;
mod event;
mod faction;
mod god;
mod item;
mod location;
mod npc;
mod reference;

/// Stateless namespace for all seed/story generators. Inherent `impl` blocks for
/// this type are split across the per-kind submodules.
pub struct AiGenerationService;

pub use dungeon::*;
pub use engine::*;
pub use faction::*;
pub use god::*;
pub use location::*;
pub use npc::*;

pub(crate) use dungeon::anchor_mechanic;
pub(crate) use engine::parse_recent_seeds;
pub(crate) use faction::build_faction_wizard_user_prompt;
pub(crate) use location::build_wizard_user_prompt;
pub(crate) use npc::{
    describe_recent_npc_occupation_anchors, occupation_anchor, recent_occupation_anchor_set,
};
pub(crate) use reference::build_reference_context;
