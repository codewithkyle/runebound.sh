use super::kind::EntityKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Text,
    Enum,
    List,
    IntegerLikeText,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldAccess {
    Set,
    Reroll,
}

impl FieldAccess {
    const fn allows(self, spec: &EntityFieldSpec) -> bool {
        match self {
            FieldAccess::Set => spec.settable,
            FieldAccess::Reroll => spec.rerollable,
        }
    }
}

#[derive(Debug)]
pub struct EntityFieldSpec {
    pub canonical: &'static str,
    pub display_name: &'static str,
    pub aliases: &'static [&'static str],
    /// One-line description shown in `<entity> set help` / `<entity> reroll help`.
    pub description: &'static str,
    #[allow(dead_code)]
    pub value_kind: ValueKind,
    pub settable: bool,
    pub rerollable: bool,
    /// Imperative instruction handed to the LLM when this field is rerolled
    /// (`<entity> reroll <field>`). Single-sources what `EntityRerollService` used
    /// to inline as a per-kind `field_instructions` match (P5.2c). Empty for
    /// non-rerollable fields (they are never sent to the model).
    pub reroll_instruction: &'static str,
}

impl EntityFieldSpec {
    fn matches(&self, candidate: &str) -> bool {
        self.canonical == candidate || self.aliases.contains(&candidate)
    }
}

pub struct EntitySchema {
    #[allow(dead_code)]
    pub kind: EntityKind,
    pub fields: &'static [EntityFieldSpec],
}

const NPC_FIELDS: [EntityFieldSpec; 11] = [
    EntityFieldSpec {
        canonical: "name",
        display_name: "name",
        aliases: &["name"],
        description: "Full name of the NPC.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a single fitting fantasy NPC name.",
    },
    EntityFieldSpec {
        canonical: "race",
        display_name: "race",
        aliases: &["race"],
        description: "Ancestry or species (e.g. Human, Elf, Dwarf).",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a fitting fantasy race for this NPC.",
    },
    EntityFieldSpec {
        canonical: "occupation",
        display_name: "occupation",
        aliases: &["occupation"],
        description: "Role, job, or trade.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate one concise occupation for this NPC.",
    },
    EntityFieldSpec {
        canonical: "sex",
        display_name: "sex",
        aliases: &["sex"],
        description: "Biological sex: male or female.",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate sex as exactly male or female.",
    },
    EntityFieldSpec {
        canonical: "age",
        display_name: "age",
        aliases: &["age"],
        description: "Age in years.",
        value_kind: ValueKind::IntegerLikeText,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a concise age value (typically in years).",
    },
    EntityFieldSpec {
        canonical: "height",
        display_name: "height",
        aliases: &["height"],
        description: "Height, e.g. 5'11\".",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a height in imperial format like 5'11\".",
    },
    EntityFieldSpec {
        canonical: "weight_lbs",
        display_name: "weight",
        aliases: &["weight", "weight_lbs"],
        description: "Weight in pounds.",
        value_kind: ValueKind::IntegerLikeText,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a weight in lbs as text, for example 185.",
    },
    EntityFieldSpec {
        canonical: "background",
        display_name: "background",
        aliases: &["background"],
        description: "1-3 sentences of personal history.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a coherent background in 1-3 sentences.",
    },
    EntityFieldSpec {
        canonical: "want_need",
        display_name: "want",
        aliases: &["want", "need", "want_need"],
        description: "What the NPC openly wants or needs.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate one concise Want.",
    },
    EntityFieldSpec {
        canonical: "secret_obstacle",
        display_name: "secret",
        aliases: &["secret", "obstacle", "secret_obstacle"],
        description: "A hidden secret or obstacle.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate one concise Secret.",
    },
    EntityFieldSpec {
        canonical: "carrying",
        display_name: "carrying",
        aliases: &["carrying"],
        description: "Notable items the NPC carries (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a carrying list as practical comma-like item strings.",
    },
];

const LOCATION_FIELDS: [EntityFieldSpec; 10] = [
    EntityFieldSpec {
        canonical: "name",
        display_name: "name",
        aliases: &["name"],
        description: "Name of the location.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a concise, fitting fantasy location name.",
    },
    EntityFieldSpec {
        canonical: "kind_type",
        display_name: "kind",
        aliases: &["kind", "kind_type"],
        description: "Location type (hamlet, town, city, hideout, ...).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate one kind_type enum value from: hamlet, town, city, hideout, ruin, guildhall, landmark, wilderness, other.",
    },
    EntityFieldSpec {
        canonical: "kind_custom",
        display_name: "kind_custom",
        aliases: &["kind_custom", "custom_kind"],
        description: "Custom type label when kind is 'other'.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a concise custom kind label for this location.",
    },
    EntityFieldSpec {
        canonical: "visual_description",
        display_name: "visual",
        aliases: &["visual", "visual_description", "description"],
        description: "What the place looks like (1-3 sentences).",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a visual description in 1-3 sentences.",
    },
    EntityFieldSpec {
        canonical: "history_background",
        display_name: "history",
        aliases: &["history", "history_background", "background"],
        description: "History and background (2-5 sentences).",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a history/background in 2-5 sentences.",
    },
    EntityFieldSpec {
        canonical: "exports",
        display_name: "exports",
        aliases: &["exports"],
        description: "Notable goods or specialties (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate 1-3 exports as concise industry or specialty item strings.",
    },
    EntityFieldSpec {
        canonical: "tone",
        display_name: "tone",
        aliases: &["tone"],
        description: "Overall mood in a few words.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a mood tone in 2-5 words.",
    },
    EntityFieldSpec {
        canonical: "authority",
        display_name: "authority",
        aliases: &["authority"],
        description: "Who governs or controls the location.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate who controls or governs this location.",
    },
    EntityFieldSpec {
        canonical: "danger_level",
        display_name: "danger",
        aliases: &["danger", "danger_level"],
        description: "Danger level (safe, guarded, risky, deadly).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate danger_level as one of: Unknown, safe, guarded, risky, deadly.",
    },
    EntityFieldSpec {
        canonical: "current_tension",
        display_name: "tension",
        aliases: &["tension", "current_tension"],
        description: "The current conflict or tension.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate current_tension in 1-2 sentences.",
    },
];

// The WOAC + visible-face fields are LLM-rerollable; the relational/place fields
// (leader, allies, rivals, liege, loyalty_type) are settable but NEVER rerollable
// — they are wizard-picked or left blank, never invented by the model (design §7,
// spec D3). `category` is intentionally absent: it is derived from `kind_type` at
// save (D2), not an editable field. Old aliases (agenda/tension/methods/leadership)
// stay pointed at the new fields so muscle-memory commands still resolve.
const FACTION_FIELDS: [EntityFieldSpec; 16] = [
    EntityFieldSpec {
        canonical: "name",
        display_name: "name",
        aliases: &["name"],
        description: "Name of the faction.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a concise fantasy faction name.",
    },
    EntityFieldSpec {
        canonical: "kind_type",
        display_name: "kind",
        aliases: &["kind", "kind_type"],
        description: "Faction kind (great_house, guild, temple, ...).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate one kind_type enum value from: great_house, major_vassal, minor_vassal, individual_lord, guild, company, criminal_syndicate, temple, cult.",
    },
    EntityFieldSpec {
        canonical: "public_description",
        display_name: "public",
        aliases: &["public", "public_description"],
        description: "How the faction presents itself publicly.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate the faction's public-facing face in 1-3 sentences — the claim it makes, not the leverage behind it.",
    },
    EntityFieldSpec {
        canonical: "reputation",
        display_name: "reputation",
        aliases: &["reputation"],
        description: "How others regard them.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate how others regard the faction in 1-2 sentences.",
    },
    EntityFieldSpec {
        canonical: "symbol_description",
        display_name: "symbol",
        aliases: &["symbol", "sigil", "banner", "symbol_description"],
        description: "Their symbol, sigil, or banner.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate exactly 1 sentence describing the faction's symbol/sigil/colors/banner/iconography.",
    },
    EntityFieldSpec {
        canonical: "want",
        display_name: "want",
        aliases: &["want", "agenda"],
        description: "Their deep aim (WOAC Want).",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate the faction's deep aim (its Want) in 1-2 sentences.",
    },
    EntityFieldSpec {
        canonical: "obstacle",
        display_name: "obstacle",
        aliases: &["obstacle", "tension"],
        description: "What stands in their way (WOAC Obstacle).",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate the obstacle in the faction's way (its Obstacle) in 1-2 sentences.",
    },
    EntityFieldSpec {
        canonical: "action",
        display_name: "action",
        aliases: &["action", "methods"],
        description: "How they pursue their Want (WOAC Action).",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate how the faction pursues its Want (its Action) in 1-2 sentences.",
    },
    EntityFieldSpec {
        canonical: "consequence",
        display_name: "consequence",
        aliases: &["consequence"],
        description: "The hook that lands on the table (WOAC Consequence).",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate the consequence that lands on the table if the faction acts (its Consequence) in 1-2 sentences.",
    },
    EntityFieldSpec {
        canonical: "sphere_of_influence",
        display_name: "influence",
        aliases: &["influence", "sphere_of_influence"],
        description: "Their sphere of influence.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate the faction's sphere of influence in 1-2 sentences, scaled to its reach.",
    },
    EntityFieldSpec {
        canonical: "resources_assets",
        display_name: "resources",
        aliases: &["resources", "resources_assets"],
        description: "Assets and resources they command (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate 1-5 resource or asset strings the faction commands.",
    },
    EntityFieldSpec {
        canonical: "leader",
        display_name: "leader",
        aliases: &["leader", "leadership"],
        description: "Who leads the faction (NPC link or free text).",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: false,
        reroll_instruction: "",
    },
    EntityFieldSpec {
        canonical: "allies",
        display_name: "allies",
        aliases: &["allies"],
        description: "Allied factions (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: false,
        reroll_instruction: "",
    },
    EntityFieldSpec {
        canonical: "rivals_enemies",
        display_name: "rivals",
        aliases: &["rivals", "rivals_enemies"],
        description: "Rival or enemy factions (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: false,
        reroll_instruction: "",
    },
    EntityFieldSpec {
        canonical: "liege",
        display_name: "liege",
        aliases: &["liege"],
        description: "The house this vassal/lord is sworn to.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: false,
        reroll_instruction: "",
    },
    EntityFieldSpec {
        canonical: "loyalty_type",
        display_name: "loyalty",
        aliases: &["loyalty", "loyalty_type"],
        description: "What binds this vassal/lord to its liege (reward, marriage, ...).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: false,
        reroll_instruction: "",
    },
];

const ITEM_FIELDS: [EntityFieldSpec; 11] = [
    EntityFieldSpec {
        canonical: "name",
        display_name: "name",
        aliases: &["name"],
        description: "Name of the item.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a concise, evocative item name.",
    },
    EntityFieldSpec {
        canonical: "category",
        display_name: "category",
        aliases: &["category", "type"],
        description: "Item category (weapon, armor, wondrous, ...).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate one category from: weapon, armor, consumable, wondrous, arcane_focus, tool, trinket, other.",
    },
    EntityFieldSpec {
        canonical: "rarity",
        display_name: "rarity",
        aliases: &["rarity"],
        description: "Rarity (common, uncommon, rare, legendary, ...).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate rarity as one of: unknown, common, uncommon, rare, very_rare, legendary, artifact.",
    },
    EntityFieldSpec {
        canonical: "attunement",
        display_name: "attunement",
        aliases: &["attune", "attunement"],
        description: "Attunement requirement, if any.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Describe attunement requirements in a short phrase (or 'None').",
    },
    EntityFieldSpec {
        canonical: "materials",
        display_name: "materials",
        aliases: &["materials"],
        description: "What the item is made of (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
        reroll_instruction: "List 1-4 notable materials as concise strings.",
    },
    EntityFieldSpec {
        canonical: "appearance",
        display_name: "appearance",
        aliases: &["appearance"],
        description: "What the item looks like.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Describe appearance in 1-2 sentences.",
    },
    EntityFieldSpec {
        canonical: "abilities",
        display_name: "abilities",
        aliases: &["abilities", "ability"],
        description: "Magical or special abilities.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Describe abilities/powers in 1-3 sentences.",
    },
    EntityFieldSpec {
        canonical: "drawbacks",
        display_name: "drawbacks",
        aliases: &["drawback", "drawbacks"],
        description: "Drawbacks or curses, if any.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Describe drawbacks/costs in up to 2 sentences (or 'None').",
    },
    EntityFieldSpec {
        canonical: "history",
        display_name: "history",
        aliases: &["history"],
        description: "Origin and history.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Describe history/origin in 1-3 sentences.",
    },
    EntityFieldSpec {
        canonical: "value",
        display_name: "value",
        aliases: &["value"],
        description: "Worth, e.g. 1000gp.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Provide estimated value in format like '1000gp' or '250sp' or '50cp' (amount + currency suffix).",
    },
    EntityFieldSpec {
        canonical: "location",
        display_name: "location",
        aliases: &["location"],
        description: "Where the item can be found.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Provide current location or hiding place.",
    },
];

const GOD_FIELDS: [EntityFieldSpec; 14] = [
    EntityFieldSpec {
        canonical: "name",
        display_name: "name",
        aliases: &["name"],
        description: "Name of the deity.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a concise fantasy deity name.",
    },
    EntityFieldSpec {
        canonical: "epithet",
        display_name: "epithet",
        aliases: &["epithet", "title"],
        description: "By-name or honorific, e.g. \"The Stormcaller\".",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a short by-name or honorific (e.g. The Stormcaller).",
    },
    // rank/alignment inline their enum values (kept in sync with GOD_RANKS /
    // GOD_ALIGNMENTS in runebound_models::utils), matching how location/faction
    // kind_type spell out their enums here.
    EntityFieldSpec {
        canonical: "rank",
        display_name: "rank",
        aliases: &["rank", "status"],
        description: "Divine rank (greater, intermediate, lesser, demigod, dead, other).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate one rank enum value from: greater, intermediate, lesser, demigod, dead, other.",
    },
    EntityFieldSpec {
        canonical: "rank_custom",
        display_name: "rank_custom",
        aliases: &["rank_custom"],
        description: "Custom rank label when rank is 'other'.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a concise custom divine rank label.",
    },
    EntityFieldSpec {
        canonical: "alignment",
        display_name: "alignment",
        aliases: &["alignment", "align"],
        description: "Moral alignment (LG, NG, CG, LN, TN, CN, LE, NE, CE).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate one alignment enum value from: LG, NG, CG, LN, TN, CN, LE, NE, CE.",
    },
    EntityFieldSpec {
        canonical: "domains",
        display_name: "domains",
        aliases: &["domains", "portfolio"],
        description: "Spheres the deity governs (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate 1-5 divine domain strings (e.g. war, death, harvest).",
    },
    EntityFieldSpec {
        canonical: "symbol",
        display_name: "symbol",
        aliases: &["symbol", "sigil", "holy_symbol"],
        description: "Holy symbol or sigil.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate exactly 1 sentence describing the holy symbol/sigil/iconography.",
    },
    EntityFieldSpec {
        canonical: "appearance",
        display_name: "appearance",
        aliases: &["appearance", "avatar"],
        description: "How the deity manifests.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate 1-3 sentences describing how the deity manifests.",
    },
    EntityFieldSpec {
        canonical: "dogma",
        display_name: "dogma",
        aliases: &["dogma", "tenets", "creed"],
        description: "Core teachings and commandments.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate core teachings/commandments in 1-3 sentences.",
    },
    EntityFieldSpec {
        canonical: "realm",
        display_name: "realm",
        aliases: &["realm", "plane"],
        description: "Home plane or divine realm.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a concise home plane or divine realm.",
    },
    EntityFieldSpec {
        canonical: "worshippers",
        display_name: "worshippers",
        aliases: &["worshippers", "followers"],
        description: "Who venerates the deity.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a concise description of who venerates the deity.",
    },
    EntityFieldSpec {
        canonical: "clergy",
        display_name: "clergy",
        aliases: &["clergy", "priesthood", "church"],
        description: "How the priesthood is organized.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a concise description of how the priesthood is organized.",
    },
    EntityFieldSpec {
        canonical: "allies",
        display_name: "allies",
        aliases: &["allies"],
        description: "Allied deities or powers (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate 1-5 allied deity or power strings.",
    },
    EntityFieldSpec {
        canonical: "rivals",
        display_name: "rivals",
        aliases: &["rivals", "enemies"],
        description: "Divine rivals and enemies (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate 1-5 rival or enemy strings.",
    },
];

// Dungeon-level scalar fields only. Beat fields are addressed compositionally
// by the domain (`dungeon set <beat> <field>`), not through this flat schema.
// topology/tone/twist are structural dials chosen in the creation flow: they are
// settable (re-pick) but not rerollable (the LLM does not invent them).
const DUNGEON_FIELDS: [EntityFieldSpec; 6] = [
    EntityFieldSpec {
        canonical: "name",
        display_name: "name",
        aliases: &["name"],
        description: "Name of the dungeon.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a concise, evocative name for the dungeon.",
    },
    EntityFieldSpec {
        canonical: "location",
        display_name: "location",
        aliases: &["location", "place"],
        description: "The single bounded place all five beats sit inside.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate the single bounded place all five beats sit inside — one short phrase naming one explorable location the party moves deeper into (e.g. 'a drowned bell-foundry'), never a region or a journey.",
    },
    EntityFieldSpec {
        canonical: "premise",
        display_name: "premise",
        aliases: &["premise", "spine"],
        description: "One-line spine summarizing the whole dungeon.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
        reroll_instruction: "Generate a single-line spine summarizing the whole dungeon (one sentence; specific but unresolved).",
    },
    EntityFieldSpec {
        canonical: "topology",
        display_name: "topology",
        aliases: &["topology", "form", "shape"],
        description: "Spatial flow shape (one of the nine forms, or none).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: false,
        reroll_instruction: "",
    },
    EntityFieldSpec {
        canonical: "tone",
        display_name: "tone",
        aliases: &["tone"],
        description: "Emotional polarity: tragedy or comedy.",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: false,
        reroll_instruction: "",
    },
    EntityFieldSpec {
        canonical: "twist",
        display_name: "twist",
        aliases: &["twist"],
        description: "Middle-beat shape: false_victory, false_defeat, or neither.",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: false,
        reroll_instruction: "",
    },
];

pub static NPC_SCHEMA: EntitySchema = EntitySchema {
    kind: EntityKind::Npc,
    fields: &NPC_FIELDS,
};

pub static LOCATION_SCHEMA: EntitySchema = EntitySchema {
    kind: EntityKind::Location,
    fields: &LOCATION_FIELDS,
};

pub static FACTION_SCHEMA: EntitySchema = EntitySchema {
    kind: EntityKind::Faction,
    fields: &FACTION_FIELDS,
};

pub static ITEM_SCHEMA: EntitySchema = EntitySchema {
    kind: EntityKind::Item,
    fields: &ITEM_FIELDS,
};

// Events are pure narrative (a title + a prose body) with no settable or
// rerollable attributes, so their schema is intentionally empty. The empty
// field list keeps `settable_fields`/`rerollable_fields` returning nothing and
// makes `set`/per-field `reroll` resolve to "no such field" for events.
pub static EVENT_SCHEMA: EntitySchema = EntitySchema {
    kind: EntityKind::Event,
    fields: &[],
};

pub static GOD_SCHEMA: EntitySchema = EntitySchema {
    kind: EntityKind::God,
    fields: &GOD_FIELDS,
};

pub static DUNGEON_SCHEMA: EntitySchema = EntitySchema {
    kind: EntityKind::Dungeon,
    fields: &DUNGEON_FIELDS,
};

pub fn schema_for_kind(kind: EntityKind) -> &'static EntitySchema {
    match kind {
        EntityKind::Npc => &NPC_SCHEMA,
        EntityKind::Location => &LOCATION_SCHEMA,
        EntityKind::Faction => &FACTION_SCHEMA,
        EntityKind::Item => &ITEM_SCHEMA,
        EntityKind::Event => &EVENT_SCHEMA,
        EntityKind::God => &GOD_SCHEMA,
        EntityKind::Dungeon => &DUNGEON_SCHEMA,
    }
}

pub fn canonical_field_spec(
    kind: EntityKind,
    raw: &str,
    access: FieldAccess,
) -> Option<&'static EntityFieldSpec> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    schema_for_kind(kind)
        .fields
        .iter()
        .find(|spec| spec.matches(&normalized) && access.allows(spec))
}

pub fn canonical_field_name(
    kind: EntityKind,
    raw: &str,
    access: FieldAccess,
) -> Option<&'static str> {
    canonical_field_spec(kind, raw, access).map(|spec| spec.canonical)
}

pub fn settable_fields(
    kind: EntityKind,
) -> impl Iterator<Item = &'static EntityFieldSpec> + 'static {
    schema_for_kind(kind)
        .fields
        .iter()
        .filter(|spec| spec.settable)
}

pub fn rerollable_fields(
    kind: EntityKind,
) -> impl Iterator<Item = &'static EntityFieldSpec> + 'static {
    schema_for_kind(kind)
        .fields
        .iter()
        .filter(|spec| spec.rerollable)
}

pub fn format_valid_field_list(kind: EntityKind, access: FieldAccess) -> String {
    let names: Vec<&'static str> = schema_for_kind(kind)
        .fields
        .iter()
        .filter(|spec| access.allows(spec))
        .map(|spec| spec.display_name)
        .collect();
    names.join(", ")
}

/// Render the `<entity> set help` / `<entity> reroll help` text from the schema:
/// usage line plus one labeled, described line per editable field (with aliases).
pub fn format_field_help(kind: EntityKind, access: FieldAccess) -> String {
    let root = kind.command_root();
    let header = match access {
        FieldAccess::Set => format!(
            "## {root} set\nUpdate a field on the active {root} draft.\nUsage: {root} set <field> <value>"
        ),
        FieldAccess::Reroll => format!(
            "## {root} reroll\nRegenerate a field on the active {root} draft with the LLM.\nUsage: {root} reroll <field> [prompt]\nThe optional prompt may include @references to vault documents."
        ),
    };

    let mut lines = vec![header, String::new(), "Fields:".to_string()];
    for spec in schema_for_kind(kind)
        .fields
        .iter()
        .filter(|spec| access.allows(spec))
    {
        let extra_aliases: Vec<&str> = spec
            .aliases
            .iter()
            .copied()
            .filter(|alias| *alias != spec.display_name)
            .collect();
        let alias_note = if extra_aliases.is_empty() {
            String::new()
        } else {
            format!(" (aliases: {})", extra_aliases.join(", "))
        };
        lines.push(format!(
            "- {} — {}{}",
            spec.display_name, spec.description, alias_note
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::super::kind::ALL_ENTITY_KINDS;
    use super::*;
    use std::collections::HashMap;

    // The entity schemas drive `set`/`reroll` field resolution, autocomplete
    // field lists, and help text. A field removed, an alias collision, or a
    // flipped settable/rerollable flag silently breaks those surfaces — these
    // tests lock the schema as a contract.

    #[test]
    fn every_alias_resolves_to_its_own_canonical_field() {
        for kind in ALL_ENTITY_KINDS {
            for spec in schema_for_kind(kind).fields {
                // The canonical name always resolves to itself.
                assert_eq!(
                    canonical_field_name(kind, spec.canonical, FieldAccess::Set),
                    Some(spec.canonical),
                    "{:?} canonical {} did not resolve",
                    kind,
                    spec.canonical,
                );
                // Every declared alias resolves back to the same canonical name.
                for alias in spec.aliases {
                    assert_eq!(
                        canonical_field_name(kind, alias, FieldAccess::Set),
                        Some(spec.canonical),
                        "{:?} alias {alias} should resolve to {}",
                        kind,
                        spec.canonical,
                    );
                }
            }
        }
    }

    #[test]
    fn alias_resolution_is_case_insensitive_and_trimmed() {
        // canonical_field_spec lowercases + trims; confirm a messy input still
        // resolves so `npc set NAME ...` / ` race ` keep working.
        assert_eq!(
            canonical_field_name(EntityKind::Npc, "  NAME  ", FieldAccess::Set),
            Some("name"),
        );
        assert_eq!(
            canonical_field_name(EntityKind::Faction, "  LOYALTY  ", FieldAccess::Set),
            Some("loyalty_type"),
        );
    }

    #[test]
    fn aliases_are_unique_within_each_entity() {
        // A single alias must never map to two different canonical fields, or
        // `set`/`reroll` resolution becomes ambiguous.
        for kind in ALL_ENTITY_KINDS {
            let mut seen: HashMap<&str, &str> = HashMap::new();
            for spec in schema_for_kind(kind).fields {
                for alias in spec.aliases {
                    if let Some(previous) = seen.insert(alias, spec.canonical) {
                        panic!(
                            "{:?} alias {alias} maps to both {previous} and {}",
                            kind, spec.canonical
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn display_names_are_unique_within_each_entity() {
        // Help/autocomplete list fields by display_name; collisions would hide a field.
        for kind in ALL_ENTITY_KINDS {
            let mut seen: HashMap<&str, &str> = HashMap::new();
            for spec in schema_for_kind(kind).fields {
                if let Some(previous) = seen.insert(spec.display_name, spec.canonical) {
                    panic!(
                        "{:?} display_name {} shared by {previous} and {}",
                        kind, spec.display_name, spec.canonical
                    );
                }
            }
        }
    }

    #[test]
    fn unknown_and_empty_fields_do_not_resolve() {
        for kind in ALL_ENTITY_KINDS {
            assert_eq!(canonical_field_name(kind, "", FieldAccess::Set), None);
            assert_eq!(canonical_field_name(kind, "   ", FieldAccess::Set), None);
            assert_eq!(
                canonical_field_name(kind, "definitely_not_a_field", FieldAccess::Set),
                None,
            );
        }
    }

    #[test]
    fn field_access_gates_resolution() {
        // canonical_field_spec only resolves a field when the requested access
        // is allowed. Locks the invariant: a field is reachable via an access
        // mode exactly when its flag for that mode is set. Most entities mark
        // every field both settable and rerollable; the dungeon deliberately
        // diverges (topology/tone/twist are settable dials, not rerollable).
        for kind in ALL_ENTITY_KINDS {
            for spec in schema_for_kind(kind).fields {
                assert_eq!(
                    canonical_field_spec(kind, spec.canonical, FieldAccess::Set).is_some(),
                    spec.settable,
                    "{:?} field {} Set-reachability should match settable flag",
                    kind,
                    spec.canonical,
                );
                assert_eq!(
                    canonical_field_spec(kind, spec.canonical, FieldAccess::Reroll).is_some(),
                    spec.rerollable,
                    "{:?} field {} Reroll-reachability should match rerollable flag",
                    kind,
                    spec.canonical,
                );
            }
        }
    }

    #[test]
    fn settable_and_rerollable_field_counts_are_locked() {
        // Snapshot the editable surface per entity as (settable, rerollable).
        // Adding/removing a field is a deliberate change that should update this
        // assertion. The dungeon's settable and rerollable counts differ on
        // purpose: topology/tone/twist are settable dials but not rerollable.
        let expected = [
            (EntityKind::Npc, 11usize, 11usize),
            (EntityKind::Location, 10, 10),
            // Faction: all 16 fields settable, but the 5 relational/place fields
            // (leader, allies, rivals, liege, loyalty_type) are never rerollable (D3).
            (EntityKind::Faction, 16, 11),
            (EntityKind::Item, 11, 11),
            // Events are narrative-only: no settable or rerollable fields.
            (EntityKind::Event, 0, 0),
            (EntityKind::God, 14, 14),
            (EntityKind::Dungeon, 6, 3),
        ];
        for (kind, settable, rerollable) in expected {
            assert_eq!(
                settable_fields(kind).count(),
                settable,
                "{:?} settable field count changed",
                kind
            );
            assert_eq!(
                rerollable_fields(kind).count(),
                rerollable,
                "{:?} rerollable field count changed",
                kind
            );
        }
    }

    #[test]
    fn valid_field_lists_are_non_empty_for_both_accesses() {
        for kind in ALL_ENTITY_KINDS {
            // Events are narrative-only and intentionally have no editable
            // fields, so an empty field list is correct for them.
            if kind == EntityKind::Event {
                assert!(format_valid_field_list(kind, FieldAccess::Set).is_empty());
                continue;
            }
            assert!(!format_valid_field_list(kind, FieldAccess::Set).is_empty());
            assert!(!format_valid_field_list(kind, FieldAccess::Reroll).is_empty());
        }
    }

    #[test]
    fn known_aliases_resolve_to_expected_canonicals() {
        // Spot-check the renamed-canonical aliases that callers and docs rely
        // on; these are the easy ones to break in a refactor.
        let cases = [
            (EntityKind::Npc, "weight", "weight_lbs"),
            (EntityKind::Npc, "want", "want_need"),
            (EntityKind::Npc, "secret", "secret_obstacle"),
            (EntityKind::Location, "kind", "kind_type"),
            (EntityKind::Location, "danger", "danger_level"),
            // The kept muscle-memory aliases now point at their WOAC/renamed fields.
            (EntityKind::Faction, "agenda", "want"),
            (EntityKind::Faction, "tension", "obstacle"),
            (EntityKind::Faction, "methods", "action"),
            (EntityKind::Faction, "leadership", "leader"),
            (EntityKind::Faction, "symbol", "symbol_description"),
            (EntityKind::Item, "type", "category"),
        ];
        for (kind, alias, canonical) in cases {
            assert_eq!(
                canonical_field_name(kind, alias, FieldAccess::Set),
                Some(canonical),
                "{:?} alias {alias} should resolve to {canonical}",
                kind,
            );
        }
    }
}
