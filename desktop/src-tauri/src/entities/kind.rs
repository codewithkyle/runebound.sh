use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Npc,
    Location,
    Faction,
    Item,
    Event,
    God,
    Dungeon,
}

#[allow(dead_code)]
impl EntityKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            EntityKind::Npc => "npc",
            EntityKind::Location => "location",
            EntityKind::Faction => "faction",
            EntityKind::Item => "item",
            EntityKind::Event => "event",
            EntityKind::God => "god",
            EntityKind::Dungeon => "dungeon",
        }
    }

    pub const fn command_root(self) -> &'static str {
        self.as_str()
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            EntityKind::Npc => "NPC",
            EntityKind::Location => "Location",
            EntityKind::Faction => "Faction",
            EntityKind::Item => "Item",
            EntityKind::Event => "Event",
            EntityKind::God => "God",
            EntityKind::Dungeon => "Dungeon",
        }
    }
}

#[allow(dead_code)]
pub const ALL_ENTITY_KINDS: [EntityKind; 7] = [
    EntityKind::Npc,
    EntityKind::Location,
    EntityKind::Faction,
    EntityKind::Item,
    EntityKind::Event,
    EntityKind::God,
    EntityKind::Dungeon,
];

#[cfg(test)]
mod tests {
    use super::*;

    // `EntityKind` replaced a duplicate `EntityType` enum that serialized
    // `snake_case`. These lock that the wire form is byte-identical to `as_str`
    // (so persisted/serialized `entity_type` values stay compatible).
    #[test]
    fn serializes_as_its_snake_case_str() {
        for kind in ALL_ENTITY_KINDS {
            let json = serde_json::to_string(&kind).expect("serialize kind");
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
        }
    }

    #[test]
    fn round_trips_through_serde() {
        for kind in ALL_ENTITY_KINDS {
            let json = serde_json::to_string(&kind).expect("serialize kind");
            let back: EntityKind = serde_json::from_str(&json).expect("deserialize kind");
            assert_eq!(back, kind);
        }
    }
}
