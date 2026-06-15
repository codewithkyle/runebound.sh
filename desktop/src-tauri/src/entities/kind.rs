#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Npc,
    Location,
    Faction,
}

#[allow(dead_code)]
impl EntityKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            EntityKind::Npc => "npc",
            EntityKind::Location => "location",
            EntityKind::Faction => "faction",
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
        }
    }
}

#[allow(dead_code)]
pub const ALL_ENTITY_KINDS: [EntityKind; 3] = [
    EntityKind::Npc,
    EntityKind::Location,
    EntityKind::Faction,
];
