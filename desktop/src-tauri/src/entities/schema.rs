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
}

impl EntityFieldSpec {
    fn matches(&self, candidate: &str) -> bool {
        self.canonical == candidate || self.aliases.iter().any(|alias| *alias == candidate)
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
    },
    EntityFieldSpec {
        canonical: "race",
        display_name: "race",
        aliases: &["race"],
        description: "Ancestry or species (e.g. Human, Elf, Dwarf).",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "occupation",
        display_name: "occupation",
        aliases: &["occupation"],
        description: "Role, job, or trade.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "sex",
        display_name: "sex",
        aliases: &["sex"],
        description: "Biological sex: male or female.",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "age",
        display_name: "age",
        aliases: &["age"],
        description: "Age in years.",
        value_kind: ValueKind::IntegerLikeText,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "height",
        display_name: "height",
        aliases: &["height"],
        description: "Height, e.g. 5'11\".",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "weight_lbs",
        display_name: "weight",
        aliases: &["weight", "weight_lbs"],
        description: "Weight in pounds.",
        value_kind: ValueKind::IntegerLikeText,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "background",
        display_name: "background",
        aliases: &["background"],
        description: "1-3 sentences of personal history.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "want_need",
        display_name: "want",
        aliases: &["want", "need", "want_need"],
        description: "What the NPC openly wants or needs.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "secret_obstacle",
        display_name: "secret",
        aliases: &["secret", "obstacle", "secret_obstacle"],
        description: "A hidden secret or obstacle.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "carrying",
        display_name: "carrying",
        aliases: &["carrying"],
        description: "Notable items the NPC carries (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
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
    },
    EntityFieldSpec {
        canonical: "kind_type",
        display_name: "kind",
        aliases: &["kind", "kind_type"],
        description: "Location type (hamlet, town, city, dungeon, ...).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "kind_custom",
        display_name: "kind_custom",
        aliases: &["kind_custom", "custom_kind"],
        description: "Custom type label when kind is 'other'.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "visual_description",
        display_name: "visual",
        aliases: &["visual", "visual_description", "description"],
        description: "What the place looks like (1-3 sentences).",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "history_background",
        display_name: "history",
        aliases: &["history", "history_background", "background"],
        description: "History and background (2-5 sentences).",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "exports",
        display_name: "exports",
        aliases: &["exports"],
        description: "Notable goods or specialties (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "tone",
        display_name: "tone",
        aliases: &["tone"],
        description: "Overall mood in a few words.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "authority",
        display_name: "authority",
        aliases: &["authority"],
        description: "Who governs or controls the location.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "danger_level",
        display_name: "danger",
        aliases: &["danger", "danger_level"],
        description: "Danger level (safe, guarded, risky, deadly).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "current_tension",
        display_name: "tension",
        aliases: &["tension", "current_tension"],
        description: "The current conflict or tension.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
];

const FACTION_FIELDS: [EntityFieldSpec; 17] = [
    EntityFieldSpec {
        canonical: "name",
        display_name: "name",
        aliases: &["name"],
        description: "Name of the faction.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "kind_type",
        display_name: "kind",
        aliases: &["kind", "kind_type"],
        description: "Faction type (guild, cult, order, ...).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "kind_custom",
        display_name: "kind_custom",
        aliases: &["kind_custom"],
        description: "Custom type label when kind is 'other'.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "public_description",
        display_name: "public",
        aliases: &["public", "public_description"],
        description: "How the faction presents itself publicly.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "true_agenda",
        display_name: "agenda",
        aliases: &["agenda", "true_agenda"],
        description: "Their true, hidden agenda.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "methods",
        display_name: "methods",
        aliases: &["methods"],
        description: "How they pursue their goals.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "leadership",
        display_name: "leadership",
        aliases: &["leadership"],
        description: "Who leads the faction.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "headquarters",
        display_name: "headquarters",
        aliases: &["headquarters", "hq"],
        description: "Their base of operations.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "sphere_of_influence",
        display_name: "influence",
        aliases: &["influence", "sphere_of_influence"],
        description: "Their sphere of influence.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "resources_assets",
        display_name: "resources",
        aliases: &["resources", "resources_assets"],
        description: "Assets and resources they command.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "allies",
        display_name: "allies",
        aliases: &["allies"],
        description: "Allied groups or individuals (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "rivals_enemies",
        display_name: "rivals",
        aliases: &["rivals", "rivals_enemies"],
        description: "Rivals and enemies (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "reputation",
        display_name: "reputation",
        aliases: &["reputation"],
        description: "How others regard them.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "current_tension",
        display_name: "tension",
        aliases: &["tension", "current_tension"],
        description: "Current internal or external tension.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "goals_short_term",
        display_name: "goals_short",
        aliases: &["goals_short", "goals_short_term"],
        description: "Short-term goals (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "goals_long_term",
        display_name: "goals_long",
        aliases: &["goals_long", "goals_long_term"],
        description: "Long-term goals (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "symbol_description",
        display_name: "symbol",
        aliases: &["symbol", "sigil", "banner", "symbol_description"],
        description: "Their symbol, sigil, or banner.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
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
    },
    EntityFieldSpec {
        canonical: "category",
        display_name: "category",
        aliases: &["category", "type"],
        description: "Item category (weapon, armor, wondrous, ...).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "rarity",
        display_name: "rarity",
        aliases: &["rarity"],
        description: "Rarity (common, uncommon, rare, legendary, ...).",
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "attunement",
        display_name: "attunement",
        aliases: &["attune", "attunement"],
        description: "Attunement requirement, if any.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "materials",
        display_name: "materials",
        aliases: &["materials"],
        description: "What the item is made of (list).",
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "appearance",
        display_name: "appearance",
        aliases: &["appearance"],
        description: "What the item looks like.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "abilities",
        display_name: "abilities",
        aliases: &["abilities", "ability"],
        description: "Magical or special abilities.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "drawbacks",
        display_name: "drawbacks",
        aliases: &["drawback", "drawbacks"],
        description: "Drawbacks or curses, if any.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "history",
        display_name: "history",
        aliases: &["history"],
        description: "Origin and history.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "value",
        display_name: "value",
        aliases: &["value"],
        description: "Worth, e.g. 1000gp.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "location",
        display_name: "location",
        aliases: &["location"],
        description: "Where the item can be found.",
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
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

pub fn schema_for_kind(kind: EntityKind) -> &'static EntitySchema {
    match kind {
        EntityKind::Npc => &NPC_SCHEMA,
        EntityKind::Location => &LOCATION_SCHEMA,
        EntityKind::Faction => &FACTION_SCHEMA,
        EntityKind::Item => &ITEM_SCHEMA,
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

    Some(
        schema_for_kind(kind)
            .fields
            .iter()
            .find(|spec| spec.matches(&normalized) && access.allows(spec))?
    )
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
            "## {root} reroll\nRegenerate a field on the active {root} draft with the LLM.\nUsage: {root} reroll <field> [prompt]"
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
