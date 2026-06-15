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
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "race",
        display_name: "race",
        aliases: &["race"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "occupation",
        display_name: "occupation",
        aliases: &["occupation"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "sex",
        display_name: "sex",
        aliases: &["sex"],
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "age",
        display_name: "age",
        aliases: &["age"],
        value_kind: ValueKind::IntegerLikeText,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "height",
        display_name: "height",
        aliases: &["height"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "weight_lbs",
        display_name: "weight",
        aliases: &["weight", "weight_lbs"],
        value_kind: ValueKind::IntegerLikeText,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "background",
        display_name: "background",
        aliases: &["background"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "want_need",
        display_name: "want",
        aliases: &["want", "need", "want_need"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "secret_obstacle",
        display_name: "secret",
        aliases: &["secret", "obstacle", "secret_obstacle"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "carrying",
        display_name: "carrying",
        aliases: &["carrying"],
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
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "kind_type",
        display_name: "kind",
        aliases: &["kind", "kind_type"],
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "kind_custom",
        display_name: "kind_custom",
        aliases: &["kind_custom", "custom_kind"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "visual_description",
        display_name: "visual",
        aliases: &["visual", "visual_description", "description"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "history_background",
        display_name: "history",
        aliases: &["history", "history_background", "background"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "exports",
        display_name: "exports",
        aliases: &["exports"],
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "tone",
        display_name: "tone",
        aliases: &["tone"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "authority",
        display_name: "authority",
        aliases: &["authority"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "danger_level",
        display_name: "danger",
        aliases: &["danger", "danger_level"],
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "current_tension",
        display_name: "tension",
        aliases: &["tension", "current_tension"],
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
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "kind_type",
        display_name: "kind",
        aliases: &["kind", "kind_type"],
        value_kind: ValueKind::Enum,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "kind_custom",
        display_name: "kind_custom",
        aliases: &["kind_custom"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "public_description",
        display_name: "public",
        aliases: &["public", "public_description"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "true_agenda",
        display_name: "agenda",
        aliases: &["agenda", "true_agenda"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "methods",
        display_name: "methods",
        aliases: &["methods"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "leadership",
        display_name: "leadership",
        aliases: &["leadership"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "headquarters",
        display_name: "headquarters",
        aliases: &["headquarters", "hq"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "sphere_of_influence",
        display_name: "influence",
        aliases: &["influence", "sphere_of_influence"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "resources_assets",
        display_name: "resources",
        aliases: &["resources", "resources_assets"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "allies",
        display_name: "allies",
        aliases: &["allies"],
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "rivals_enemies",
        display_name: "rivals",
        aliases: &["rivals", "rivals_enemies"],
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "reputation",
        display_name: "reputation",
        aliases: &["reputation"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "current_tension",
        display_name: "tension",
        aliases: &["tension", "current_tension"],
        value_kind: ValueKind::Text,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "goals_short_term",
        display_name: "goals_short",
        aliases: &["goals_short", "goals_short_term"],
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "goals_long_term",
        display_name: "goals_long",
        aliases: &["goals_long", "goals_long_term"],
        value_kind: ValueKind::List,
        settable: true,
        rerollable: true,
    },
    EntityFieldSpec {
        canonical: "symbol_description",
        display_name: "symbol",
        aliases: &["symbol", "sigil", "banner", "symbol_description"],
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

pub fn schema_for_kind(kind: EntityKind) -> &'static EntitySchema {
    match kind {
        EntityKind::Npc => &NPC_SCHEMA,
        EntityKind::Location => &LOCATION_SCHEMA,
        EntityKind::Faction => &FACTION_SCHEMA,
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
