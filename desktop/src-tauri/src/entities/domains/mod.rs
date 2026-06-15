pub mod npc_domain;
pub mod location_domain;
pub mod faction_domain;
pub mod item_domain;

pub use faction_domain::{faction_event_from_draft, faction_summary_text, FactionDomain};
pub use location_domain::{location_event_from_draft, location_summary_text, LocationDomain};
pub use npc_domain::{npc_event_from_draft, npc_summary_text, NpcDomain};
pub use item_domain::{item_event_from_draft, item_summary_text, ItemDomain};
