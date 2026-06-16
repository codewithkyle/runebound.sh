#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Npc,
    Location,
    Faction,
    Item,
    Event,
    God,
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
        }
    }
}

#[allow(dead_code)]
pub const ALL_ENTITY_KINDS: [EntityKind; 6] = [
    EntityKind::Npc,
    EntityKind::Location,
    EntityKind::Faction,
    EntityKind::Item,
    EntityKind::Event,
    EntityKind::God,
];
