//! The faction wizard: the guided `create faction` flow expressed as declarative
//! `WizardStep`s on the shared engine (docs/architecture.md §4, §8D). Step 1 picks
//! the **category** (houses / establishments / religion, D5), which routes to one of
//! three branches; houses further sub-routes by **kind** (Great House vs
//! vassal/lord). Each branch locks the GM's answers — power base, liege + loyalty,
//! control type, mandate, reach, patron, god — and ends by generating the WOAC fields
//! *under* those answers, converging on the same `FactionDraft` the one-shot
//! `create faction <prompt>` produces. So `save`/`reroll`/the card UI keep working
//! unchanged (the location wizard pattern applied to factions).
//!
//! The locked answers live only in this accumulator; they are baked into the
//! generated prose and flattened into `seed_prompt` as reroll bias. The relational
//! fields (leader, allies, rivals, liege, loyalty) are **picker-linked or left blank,
//! never generated** (design §7 / D3) — they are set here and carried straight onto
//! the draft. Nothing new is persisted (no draft/row/migration changes).

use std::sync::Arc;

use async_trait::async_trait;
use dnd_core::npc::slugify;
use runebound_models::output::{
    OutputDoc, command_ref, doc, heading, paragraph_text, paragraph_with_inlines, text_node,
};
use runebound_models::utils::{LOYALTY_TYPES, make_entity_id};

use crate::app_state::{AppState, FactionDraftSession};
use crate::commands::{faction_event_from_draft, faction_summary_text};
use crate::entities::common::{
    CommandResult, command_message_response_with_doc, command_response_with_event,
};
use crate::services::ai_generation::{
    AiGenerationService, CONTROL_TYPES, FactionSeed, FactionWizardInputs, HOUSE_BRANDS, LORD_TYPES,
    MANDATES, REACH, SeedGeneration, build_faction_wizard_user_prompt,
};
use crate::utils::prepend_notice;
use crate::wizards::entity_link::{
    EntityMatch, entity_suggestions, load_linkable_factions, load_linkable_gods, load_linkable_npcs,
    match_entity,
};

use wizard::prompt::wizard_menu;
use wizard::{Wizard, WizardChoice, WizardData, WizardStep, WizardTransition};

// ---------------------------------------------------------------------------
// Accumulator
// ---------------------------------------------------------------------------

/// The per-flow answers; the cursor/history live in the engine's `WizardSession`.
/// `seed`/`notice` carry the generated result and a one-shot capacity notice into
/// the `finalize` hand-off that opens the faction editor.
#[derive(Debug, Clone, Default)]
struct FactionWizardData {
    // Routing. `kind_type` (one of the 9) is set at the layer/kind step; the category
    // it rolls up into is derived from it (D2), never stored separately.
    kind_type: String,

    // Houses
    power_base: Option<String>,
    power_specifics: Option<String>,
    brand: Option<String>,
    /// Set after option `0` on the brand step: the next submission is the custom brand.
    awaiting_custom_brand: bool,
    liege: Option<String>,
    loyalty_type: Option<String>,

    // Establishments
    control_type: Option<String>,
    control_specifics: Option<String>,

    // Establishments + Religion
    reach: Option<String>,
    patron: Option<String>,

    // Religion
    god: Option<String>,
    mandate: Option<String>,
    mandate_specifics: Option<String>,

    // Shared tail
    want: Option<String>,
    leader: Option<String>,
    allies: Vec<String>,
    rivals: Vec<String>,
    /// The terminal generate step's optional extra detail; fed as a generation hint
    /// and persisted as reroll bias.
    detail: Option<String>,

    // Picker working sets, loaded on entry (the typeahead filters them in memory).
    factions: Vec<(String, String)>,
    npcs: Vec<(String, String)>,
    gods: Vec<(String, String)>,
    /// Which mode the shared faction picker is in (liege / patron / allies / rivals),
    /// so one step serves four link points (mirrors `faction_link_return`).
    link_return: Option<&'static str>,

    // Generated seed handed to `finalize`/the editor, plus a one-shot notice.
    seed: Option<FactionSeed>,
    notice: Option<String>,
}

fn faction_data(d: &WizardData) -> &FactionWizardData {
    d.downcast_ref::<FactionWizardData>()
        .expect("faction wizard data")
}

fn faction_data_mut(d: &mut WizardData) -> &mut FactionWizardData {
    d.downcast_mut::<FactionWizardData>()
        .expect("faction wizard data")
}

impl FactionWizardData {
    /// Project the locked answers into the generation inputs. `category` is the
    /// GM-picked branch; the rest are each branch's answers. The relational fields the
    /// LLM never sees (leader/allies/rivals) are deliberately absent (D3).
    fn as_inputs(&self) -> FactionWizardInputs {
        FactionWizardInputs {
            kind_type: self.kind_type.clone(),
            power_base: self.power_base.clone(),
            power_specifics: self.power_specifics.clone(),
            brand: self.brand.clone(),
            liege: self.liege.clone(),
            loyalty_type: self.loyalty_type.clone(),
            control_type: self.control_type.clone(),
            control_specifics: self.control_specifics.clone(),
            reach: self.reach.clone(),
            patron: self.patron.clone(),
            god: self.god.clone(),
            mandate: self.mandate.clone(),
            mandate_specifics: self.mandate_specifics.clone(),
            want: self.want.clone(),
            // The generate step's extra detail doubles as the reroll hint.
            hint: self.detail.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Step 1 — category (router, D5)
// ---------------------------------------------------------------------------

const CATEGORY_LABELS: [&str; 3] = [
    "houses (great houses, vassals, individual lords)",
    "establishments (guilds, companies, criminal syndicates)",
    "religion (temples, cults)",
];

struct CategoryStep;

#[async_trait]
impl WizardStep<AppState> for CategoryStep {
    fn id(&self) -> &'static str {
        "category"
    }

    fn summary(&self) -> &'static str {
        "Pick the faction's category (it routes the rest of the flow)."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Faction — Category",
            "What kind of power center is this?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&CATEGORY_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        _d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Ok(n) = input.trim().parse::<usize>() else {
            return Ok(WizardTransition::Stay);
        };
        // The kind (and thus the category) is locked at the next step; here we only
        // route to that category's kind step (D5).
        let next = match n {
            1 => "houses_layer",
            2 => "est_kind",
            3 => "rel_kind",
            _ => return Ok(WizardTransition::Stay),
        };
        Ok(WizardTransition::Goto(next))
    }
}

// ---------------------------------------------------------------------------
// Houses branch (design §8.1)
// ---------------------------------------------------------------------------

const HOUSE_LAYER_LABELS: [&str; 4] = [
    "great house (apex; answers to no one)",
    "major vassal (a powerful sworn house)",
    "minor vassal (a lesser sworn house)",
    "individual lord (a single sworn holding)",
];
const HOUSE_LAYER_VALUES: [&str; 4] = [
    "great_house",
    "major_vassal",
    "minor_vassal",
    "individual_lord",
];

struct HouseLayerStep;

#[async_trait]
impl WizardStep<AppState> for HouseLayerStep {
    fn id(&self) -> &'static str {
        "houses_layer"
    }

    fn summary(&self) -> &'static str {
        "Which political layer is this house? (kind is the layer — it routes the sub-flow)"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Faction — Houses — Layer",
            "Which political layer is this house?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&HOUSE_LAYER_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(kind) = pick_value(input.trim(), &HOUSE_LAYER_VALUES) else {
            return Ok(WizardTransition::Stay);
        };
        faction_data_mut(d).kind_type = kind.to_string();
        Ok(WizardTransition::Goto("power_base"))
    }
}

const POWER_BASE_LABELS: [&str; 6] = [
    "chokepoint — a pass, strait, or road bottleneck (tolls)",
    "surplus — granaries, warehouses, distribution",
    "junction — a transport interchange (transfer fees)",
    "specialist — refining goods for value before shipping",
    "march — defending the realm's edge (delegated autonomy)",
    "extraction — a point-source of ore, salt, or stone",
];

struct PowerBaseStep;

#[async_trait]
impl WizardStep<AppState> for PowerBaseStep {
    fn id(&self) -> &'static str {
        "power_base"
    }

    fn summary(&self) -> &'static str {
        "What logistics problem does this house solve? (seeds its obstacle + wealth)"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Faction — Houses — Power Base",
            "Where does this house's power come from?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        // No random: the GM has already read the logistics off their map (design §8.1).
        numbered_choices(&POWER_BASE_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(base) = pick_value(input.trim(), &LORD_TYPES) else {
            return Ok(WizardTransition::Stay);
        };
        faction_data_mut(d).power_base = Some(base.to_string());
        Ok(WizardTransition::Goto("power_specifics"))
    }
}

/// After power-specifics, a Great House goes to its brand step; a vassal/lord goes to
/// the (mandatory) liege picker. Pure, so the split is unit-testable.
fn power_specifics_next_is_brand(kind_type: &str) -> bool {
    kind_type == "great_house"
}

struct PowerSpecificsStep;

#[async_trait]
impl WizardStep<AppState> for PowerSpecificsStep {
    fn id(&self) -> &'static str {
        "power_specifics"
    }

    fn summary(&self) -> &'static str {
        "Optional: name the resource, route, or holding. Type it, or skip."
    }

    fn prompt(&self, _data: &WizardData) -> OutputDoc {
        optional_text_prompt(
            "Create Faction — Houses — Specifics",
            "Name the resource, route, or holding (e.g. \"the only bridge over the Ironwash\", \"silver and salt\").",
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![skip_choice("Let the model fill in the holding")]
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        faction_data_mut(d).power_specifics = optional_text(input);
        if power_specifics_next_is_brand(&faction_data(d).kind_type) {
            Ok(WizardTransition::Goto("brand"))
        } else {
            // Vassal / lord: who are they sworn to? (mandatory liege picker)
            enter_faction_pick(d, state, "liege").await
        }
    }
}

struct BrandStep;

#[async_trait]
impl WizardStep<AppState> for BrandStep {
    fn id(&self) -> &'static str {
        "brand"
    }

    fn summary(&self) -> &'static str {
        "What is this Great House known for? Pick one, or 0 to name your own."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        if faction_data(data).awaiting_custom_brand {
            return doc()
                .with_block(heading(2, "Create Faction — Houses — Brand"))
                .with_block(paragraph_text("Type what this house is known for."));
        }
        wizard_menu(
            "Create Faction — Houses — Brand",
            "What is this house known for above all?",
            &self.choices(data),
        )
    }

    fn choices(&self, data: &WizardData) -> Vec<WizardChoice> {
        if faction_data(data).awaiting_custom_brand {
            return Vec::new();
        }
        let mut choices = vec![
            WizardChoice::new("0: custom / type your own", "0")
                .with_help("Name what they're known for"),
        ];
        choices.extend(numbered_choices(&brand_labels()));
        choices
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();

        if faction_data(d).awaiting_custom_brand {
            if trimmed.is_empty() {
                return Ok(WizardTransition::Stay);
            }
            let data = faction_data_mut(d);
            data.brand = Some(trimmed.to_string());
            data.awaiting_custom_brand = false;
            return Ok(WizardTransition::Goto("ambition"));
        }

        if trimmed == "0" {
            faction_data_mut(d).awaiting_custom_brand = true;
            return Ok(WizardTransition::Stay);
        }

        let labels = brand_labels();
        let Some(brand) = pick_value(trimmed, &labels) else {
            return Ok(WizardTransition::Stay);
        };
        faction_data_mut(d).brand = Some(brand.to_string());
        Ok(WizardTransition::Goto("ambition"))
    }
}

const LOYALTY_LABELS: [&str; 7] = [
    "reward — land, titles, or payment",
    "marriage — a blood or alliance bond",
    "military — protection, or the threat of force",
    "economic — debt or trade dependence",
    "shared enemy — a common threat",
    "oath — a sworn word",
    "secret — mutual blackmail",
];

struct LoyaltyTypeStep;

#[async_trait]
impl WizardStep<AppState> for LoyaltyTypeStep {
    fn id(&self) -> &'static str {
        "loyalty_type"
    }

    fn summary(&self) -> &'static str {
        "What binds this house to its liege? Pick one, or 0 for random."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Faction — Houses — Loyalty",
            "What binds this house to its liege?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        let mut choices =
            vec![WizardChoice::new("0: random", "0").with_help("Pick a loyalty type at random")];
        choices.extend(numbered_choices(&LOYALTY_LABELS));
        choices
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        // `0` = random; it always resolves to a value (design §6).
        let value = if trimmed == "0" {
            random_loyalty().to_string()
        } else if let Some(value) = pick_value(trimmed, &LOYALTY_TYPES) {
            value.to_string()
        } else {
            return Ok(WizardTransition::Stay);
        };
        faction_data_mut(d).loyalty_type = Some(value);
        Ok(WizardTransition::Goto("ambition"))
    }
}

// ---------------------------------------------------------------------------
// Establishments branch (design §8.2)
// ---------------------------------------------------------------------------

const EST_KIND_LABELS: [&str; 3] = [
    "guild (a craft or trade body — legit)",
    "company (mercenaries, merchants, chartered ventures)",
    "criminal syndicate (illicit; wide public/true gap)",
];
const EST_KIND_VALUES: [&str; 3] = ["guild", "company", "criminal_syndicate"];

struct EstKindStep;

#[async_trait]
impl WizardStep<AppState> for EstKindStep {
    fn id(&self) -> &'static str {
        "est_kind"
    }

    fn summary(&self) -> &'static str {
        "Which kind of establishment? (kind sets the legit-vs-illicit tone)"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Faction — Establishment — Kind",
            "Which kind of establishment is this?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&EST_KIND_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(kind) = pick_value(input.trim(), &EST_KIND_VALUES) else {
            return Ok(WizardTransition::Stay);
        };
        faction_data_mut(d).kind_type = kind.to_string();
        Ok(WizardTransition::Goto("control_type"))
    }
}

const CONTROL_LABELS: [&str; 5] = [
    "craft / good — smiths, alchemists, masons",
    "service / force — mercenaries, assassins, spies",
    "trade / transport — caravans, shipping, brokers",
    "vice / contraband — smuggling, gambling, theft",
    "knowledge / influence — spymasters, fixers, lenders",
];

struct ControlTypeStep;

#[async_trait]
impl WizardStep<AppState> for ControlTypeStep {
    fn id(&self) -> &'static str {
        "control_type"
    }

    fn summary(&self) -> &'static str {
        "What does this establishment control? (seeds its obstacle)"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Faction — Establishment — Control",
            "What does this establishment control?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&CONTROL_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(control) = pick_value(input.trim(), &CONTROL_TYPES) else {
            return Ok(WizardTransition::Stay);
        };
        faction_data_mut(d).control_type = Some(control.to_string());
        Ok(WizardTransition::Goto("control_specifics"))
    }
}

struct ControlSpecificsStep;

#[async_trait]
impl WizardStep<AppState> for ControlSpecificsStep {
    fn id(&self) -> &'static str {
        "control_specifics"
    }

    fn summary(&self) -> &'static str {
        "Optional: refine what they control. Type it, or skip."
    }

    fn prompt(&self, _data: &WizardData) -> OutputDoc {
        optional_text_prompt(
            "Create Faction — Establishment — Specifics",
            "Refine what they control (e.g. \"iron, bronze, and steel smithing\").",
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![skip_choice("Let the model fill in the specifics")]
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        faction_data_mut(d).control_specifics = optional_text(input);
        Ok(WizardTransition::Goto("reach"))
    }
}

// ---------------------------------------------------------------------------
// Religion branch (design §8.3)
// ---------------------------------------------------------------------------

const REL_KIND_LABELS: [&str; 2] = [
    "temple (a public faith — narrow public/true gap)",
    "cult (a hidden creed — wide public/true gap)",
];
const REL_KIND_VALUES: [&str; 2] = ["temple", "cult"];

struct RelKindStep;

#[async_trait]
impl WizardStep<AppState> for RelKindStep {
    fn id(&self) -> &'static str {
        "rel_kind"
    }

    fn summary(&self) -> &'static str {
        "Temple or cult? (kind sets the tone — public faith vs hidden creed)"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Faction — Religion — Kind",
            "Is this a temple or a cult?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&REL_KIND_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(kind) = pick_value(input.trim(), &REL_KIND_VALUES) else {
            return Ok(WizardTransition::Stay);
        };
        faction_data_mut(d).kind_type = kind.to_string();
        // The god is mandatory and runs next, so open straight on its picker.
        enter_god_pick(d, state).await
    }
}

const MANDATE_LABELS: [&str; 6] = [
    "devotion & tribute (worship, offerings)",
    "sacrifice (blood, lives, valuables)",
    "conquest & conversion (spread the faith)",
    "purity & law (enforce a moral order)",
    "secret knowledge (forbidden lore)",
    "cycle & nature (death/rebirth, seasons, wilds)",
];

struct MandateStep;

#[async_trait]
impl WizardStep<AppState> for MandateStep {
    fn id(&self) -> &'static str {
        "mandate"
    }

    fn summary(&self) -> &'static str {
        "What does the god demand? (seeds the obstacle, colored by kind)"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Faction — Religion — Mandate",
            "What does the god demand?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&MANDATE_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(mandate) = pick_value(input.trim(), &MANDATES) else {
            return Ok(WizardTransition::Stay);
        };
        faction_data_mut(d).mandate = Some(mandate.to_string());
        Ok(WizardTransition::Goto("mandate_specifics"))
    }
}

struct MandateSpecificsStep;

#[async_trait]
impl WizardStep<AppState> for MandateSpecificsStep {
    fn id(&self) -> &'static str {
        "mandate_specifics"
    }

    fn summary(&self) -> &'static str {
        "Optional: sharpen what the god demands. Type it, or skip."
    }

    fn prompt(&self, _data: &WizardData) -> OutputDoc {
        optional_text_prompt(
            "Create Faction — Religion — Specifics",
            "Sharpen what the god demands (e.g. \"midwinter blood offerings to ensure the harvest\").",
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![skip_choice("Let the model fill in the specifics")]
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        faction_data_mut(d).mandate_specifics = optional_text(input);
        Ok(WizardTransition::Goto("reach"))
    }
}

// ---------------------------------------------------------------------------
// Shared reach (establishments + religion)
// ---------------------------------------------------------------------------

const REACH_LABELS: [&str; 3] = [
    "local — one town, valley, or quarter",
    "regional — several settlements or a province",
    "realm-spanning",
];

struct ReachStep;

#[async_trait]
impl WizardStep<AppState> for ReachStep {
    fn id(&self) -> &'static str {
        "reach"
    }

    fn summary(&self) -> &'static str {
        "How far does it reach? (scales its sphere of influence)"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Faction — Reach",
            "How far does it reach?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&REACH_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(reach) = pick_value(input.trim(), &REACH) else {
            return Ok(WizardTransition::Stay);
        };
        faction_data_mut(d).reach = Some(reach.to_string());
        // Both establishments and religion ask for an optional patron next.
        enter_faction_pick(d, state, "patron").await
    }
}

// ---------------------------------------------------------------------------
// Shared tail — ambition → leader → allies → rivals → generate
// ---------------------------------------------------------------------------

struct AmbitionStep;

#[async_trait]
impl WizardStep<AppState> for AmbitionStep {
    fn id(&self) -> &'static str {
        "ambition"
    }

    fn summary(&self) -> &'static str {
        "Optional: the faction's deep aim (WOAC Want). Type it, or skip to let the model infer it."
    }

    fn prompt(&self, _data: &WizardData) -> OutputDoc {
        optional_text_prompt(
            "Create Faction — Ambition",
            "What does this faction ultimately want? (its WOAC Want).",
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![skip_choice("Let the model infer the Want from the locked answers")]
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        faction_data_mut(d).want = optional_text(input);
        // The leader is an NPC link, so open its picker next.
        enter_npc_pick(d, state).await
    }
}

/// The terminal step: record the (optional) extra detail, run generation under the
/// locked answers, then complete — `finalize` opens the draft straight in the faction
/// editor (mirroring the one-shot `create_faction`; no separate review screen).
struct GenerateStep;

#[async_trait]
impl WizardStep<AppState> for GenerateStep {
    fn id(&self) -> &'static str {
        "generate"
    }

    fn summary(&self) -> &'static str {
        "Optional free text, then generate. Type a detail, or skip to generate now."
    }

    fn awaiting_llm_label(&self) -> Option<&'static str> {
        Some("generating faction")
    }

    fn prompt(&self, _data: &WizardData) -> OutputDoc {
        doc()
            .with_block(heading(2, "Create Faction — Generate"))
            .with_block(paragraph_with_inlines(vec![
                text_node("Add any last detail to steer the generation. Or "),
                command_ref("skip", "skip"),
                text_node(" to generate now."),
            ]))
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![WizardChoice::new("skip", "skip").with_help("Generate without adding this")]
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        faction_data_mut(d).detail = optional_text(input);
        generate_faction_into(faction_data_mut(d), state).await?;
        Ok(WizardTransition::Complete)
    }
}

// ---------------------------------------------------------------------------
// Shared pickers
// ---------------------------------------------------------------------------

/// The shared faction picker, parameterized by `link_return` (mirrors location's
/// `FactionLinkStep`). It serves four link points:
/// - **liege** (houses vassal/lord): mandatory, free-typed name accepted → loyalty.
/// - **patron** (establishments/religion): optional grounding → ambition.
/// - **allies** / **rivals** (tail): repeatable per D4 — link one and stay to link
///   another; `done` moves on. allies then flips in place to rivals.
struct FactionPickStep;

fn faction_pick_mode(data: &FactionWizardData) -> &'static str {
    data.link_return.unwrap_or("patron")
}

#[async_trait]
impl WizardStep<AppState> for FactionPickStep {
    fn id(&self) -> &'static str {
        "faction_pick"
    }

    fn summary(&self) -> &'static str {
        "Type to search your factions by name; an unmatched name is accepted as-is."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        let d = faction_data(data);
        match faction_pick_mode(d) {
            "liege" => doc()
                .with_block(heading(2, "Create Faction — Liege"))
                .with_block(paragraph_text(
                    "Who is this house sworn to? Start typing to select a Great House, or type a new name (required).",
                )),
            "patron" => doc()
                .with_block(heading(2, "Create Faction — Patron / Charter"))
                .with_block(paragraph_with_inlines(vec![
                    text_node(
                        "Optional: which house or power charters or protects them? Type to search, or ",
                    ),
                    command_ref("skip", "skip"),
                    text_node(" for none."),
                ])),
            mode => {
                let (title, noun) = if mode == "rivals" {
                    ("Create Faction — Rivals", "a rival faction")
                } else {
                    ("Create Faction — Allies", "an ally faction")
                };
                let linked = if mode == "rivals" { &d.rivals } else { &d.allies };
                let mut document = doc().with_block(heading(2, title));
                document = document.with_block(paragraph_with_inlines(vec![
                    text_node(format!("Link {noun} (type to search), or ")),
                    command_ref("done", "done"),
                    text_node(" when finished."),
                ]));
                if !linked.is_empty() {
                    document = document
                        .with_block(paragraph_text(format!("Linked so far: {}.", linked.join(", "))));
                }
                document
            }
        }
    }

    fn choices(&self, data: &WizardData) -> Vec<WizardChoice> {
        match faction_pick_mode(faction_data(data)) {
            // Mandatory: typeahead-driven, no listed action.
            "liege" => Vec::new(),
            "patron" => vec![skip_choice("No patron; leave it open")],
            // allies / rivals: `done` finishes the repeatable picker.
            _ => vec![WizardChoice::new("done", "done").with_help("Finish linking")],
        }
    }

    fn suggest(&self, input: &str, data: &WizardData) -> Vec<WizardChoice> {
        let d = faction_data(data);
        let mut out = entity_suggestions(&d.factions, input);
        let query = input.trim().to_ascii_lowercase();
        // The trailing action depends on the mode; liege has none (mandatory).
        let action = match faction_pick_mode(d) {
            "liege" => None,
            "patron" => Some("skip"),
            _ => Some("done"),
        };
        if let Some(token) = action {
            if query.is_empty() || token.starts_with(&query) {
                out.push(WizardChoice::new(token, token));
            }
        }
        out
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        let mode = faction_pick_mode(faction_data(d));

        match mode {
            "liege" => {
                if trimmed.is_empty() {
                    // Mandatory: re-prompt rather than advancing without a liege.
                    return Ok(WizardTransition::Stay);
                }
                let name = resolve_link_name(&faction_data(d).factions, trimmed, "factions")?;
                faction_data_mut(d).liege = Some(name);
                Ok(WizardTransition::Goto("loyalty_type"))
            }
            "patron" => {
                let data = faction_data_mut(d);
                if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("skip") {
                    data.patron = None;
                } else {
                    let name = resolve_link_name(&data.factions, trimmed, "factions")?;
                    faction_data_mut(d).patron = Some(name);
                }
                Ok(WizardTransition::Goto("ambition"))
            }
            // Repeatable allies/rivals: link one and stay; `done` moves on.
            _ => {
                let finished = trimmed.is_empty()
                    || trimmed.eq_ignore_ascii_case("done")
                    || trimmed.eq_ignore_ascii_case("skip");
                if finished {
                    return Ok(if mode == "rivals" {
                        WizardTransition::Goto("generate")
                    } else {
                        // allies finished → flip the same step in place to rivals.
                        faction_data_mut(d).link_return = Some("rivals");
                        WizardTransition::Stay
                    });
                }
                let name = resolve_link_name(&faction_data(d).factions, trimmed, "factions")?;
                let data = faction_data_mut(d);
                let list = if mode == "rivals" {
                    &mut data.rivals
                } else {
                    &mut data.allies
                };
                add_link(list, &name);
                // Stay on the step to link another.
                Ok(WizardTransition::Stay)
            }
        }
    }
}

/// The NPC picker for the faction's leader (optional, free-typed name accepted).
struct NpcPickStep;

#[async_trait]
impl WizardStep<AppState> for NpcPickStep {
    fn id(&self) -> &'static str {
        "npc_pick"
    }

    fn summary(&self) -> &'static str {
        "Type to search your NPCs for the leader, type a new name, or skip."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        let body = if faction_data(data).npcs.is_empty() {
            "Who leads this faction? No NPCs exist yet — type a name, or skip to leave it blank."
        } else {
            "Who leads this faction? Start typing to select an NPC, type a new name, or skip."
        };
        doc()
            .with_block(heading(2, "Create Faction — Leader"))
            .with_block(paragraph_with_inlines(vec![
                text_node(body),
                text_node(" Use "),
                command_ref("skip", "skip"),
                text_node(" to leave the leadership section blank."),
            ]))
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![skip_choice("Leave the leadership section blank")]
    }

    fn suggest(&self, input: &str, data: &WizardData) -> Vec<WizardChoice> {
        let d = faction_data(data);
        let mut out = entity_suggestions(&d.npcs, input);
        let query = input.trim().to_ascii_lowercase();
        if query.is_empty() || "skip".starts_with(&query) {
            out.push(WizardChoice::new("skip", "skip"));
        }
        out
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("skip") {
            faction_data_mut(d).leader = None;
        } else {
            let name = resolve_link_name(&faction_data(d).npcs, trimmed, "npcs")?;
            faction_data_mut(d).leader = Some(name);
        }
        // Leader done → the (repeatable) allies picker.
        enter_faction_pick(d, state, "allies").await
    }
}

/// The god picker for a temple/cult (mandatory, free-typed name accepted).
struct GodPickStep;

#[async_trait]
impl WizardStep<AppState> for GodPickStep {
    fn id(&self) -> &'static str {
        "god_pick"
    }

    fn summary(&self) -> &'static str {
        "Type to search your gods, or type a new name (required for a temple or cult)."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        let body = if faction_data(data).gods.is_empty() {
            "Which god does this faith serve? No gods exist yet — type a name."
        } else {
            "Which god does this faith serve? Start typing to select a god, or type a new name."
        };
        doc()
            .with_block(heading(2, "Create Faction — God"))
            .with_block(paragraph_text(body))
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        // Mandatory and typeahead-driven, so no listed action (and no `skip`).
        Vec::new()
    }

    fn suggest(&self, input: &str, data: &WizardData) -> Vec<WizardChoice> {
        entity_suggestions(&faction_data(data).gods, input)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            // Mandatory: a temple/cult serves *someone*.
            return Ok(WizardTransition::Stay);
        }
        let name = resolve_link_name(&faction_data(d).gods, trimmed, "gods")?;
        faction_data_mut(d).god = Some(name);
        Ok(WizardTransition::Goto("mandate"))
    }
}

// ---------------------------------------------------------------------------
// Wizard
// ---------------------------------------------------------------------------

pub struct FactionWizard {
    steps: Vec<Arc<dyn WizardStep<AppState>>>,
}

impl FactionWizard {
    pub fn new() -> Self {
        Self {
            steps: vec![
                Arc::new(CategoryStep),
                // Houses
                Arc::new(HouseLayerStep),
                Arc::new(PowerBaseStep),
                Arc::new(PowerSpecificsStep),
                Arc::new(BrandStep),
                Arc::new(LoyaltyTypeStep),
                // Establishments
                Arc::new(EstKindStep),
                Arc::new(ControlTypeStep),
                Arc::new(ControlSpecificsStep),
                // Religion
                Arc::new(RelKindStep),
                Arc::new(MandateStep),
                Arc::new(MandateSpecificsStep),
                // Shared reach (establishments + religion)
                Arc::new(ReachStep),
                // Shared tail
                Arc::new(AmbitionStep),
                Arc::new(GenerateStep),
                // Shared pickers (parameterized by link_return / loaded on entry)
                Arc::new(FactionPickStep),
                Arc::new(NpcPickStep),
                Arc::new(GodPickStep),
            ],
        }
    }
}

#[async_trait]
impl Wizard<AppState> for FactionWizard {
    fn id(&self) -> &'static str {
        "faction"
    }

    fn title(&self) -> &'static str {
        "Create Faction"
    }

    fn steps(&self) -> &[Arc<dyn WizardStep<AppState>>] {
        &self.steps
    }

    async fn seed(&self, _host: &AppState) -> Result<WizardData, String> {
        Ok(WizardData::new(FactionWizardData::default()))
    }

    /// Runs when the generate step completes: build the `FactionDraft` from the
    /// generated seed + locked answers and open it in the faction editor — the same
    /// hand-off the one-shot `create_faction` performs. No LLM call here.
    async fn finalize(&self, state: &AppState, d: &WizardData) -> CommandResult {
        let data = faction_data(d);
        let Some(draft) = build_faction_draft(data, make_entity_id("fac")) else {
            return command_message_response_with_doc(
                "faction flow reset.",
                doc().with_block(paragraph_text(
                    "Faction flow reset; run create faction again.",
                )),
            );
        };

        {
            let mut editor = state.editor_session.lock().await;
            editor.set_faction(draft.clone());
        }

        command_response_with_event(
            prepend_notice(data.notice.clone(), faction_summary_text(&draft)),
            faction_event_from_draft(&draft),
        )
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Numbered, clickable choices from a label slice: `["a","b"]` → `1: a`/`2: b` with
/// tokens `1`/`2`. (Duplicated from the location wizard per spec §5.3.)
fn numbered_choices(labels: &[&str]) -> Vec<WizardChoice> {
    labels
        .iter()
        .enumerate()
        .map(|(i, label)| WizardChoice::new(format!("{}: {label}", i + 1), (i + 1).to_string()))
        .collect()
}

/// Map a numeric token (`1`-based) to its value in a parallel slice. (Duplicated from
/// the location wizard per spec §5.3.)
fn pick_value<'a>(input: &str, values: &[&'a str]) -> Option<&'a str> {
    let n = input.parse::<usize>().ok()?;
    if (1..=values.len()).contains(&n) {
        Some(values[n - 1])
    } else {
        None
    }
}

/// The shared `skip` action choice.
fn skip_choice(help: &'static str) -> WizardChoice {
    WizardChoice::new("skip", "skip").with_help(help)
}

/// An optional-free-text step's prompt: a heading, the body, and a `skip` link.
fn optional_text_prompt(title: &str, body: &str) -> OutputDoc {
    doc()
        .with_block(heading(2, title))
        .with_block(paragraph_with_inlines(vec![
            text_node(format!("{body} Or ")),
            command_ref("skip", "skip"),
            text_node(" to move on."),
        ]))
}

/// Normalize an optional-text submission: trim, and treat empty / `skip` as `None`.
fn optional_text(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("skip") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Resolve a submitted link name against a loaded `(name, slug)` set: an exact/unique
/// match resolves to its canonical name; an unmatched name is accepted as free text
/// (it renders as a `[[wikilink]]` that resolves by name in Obsidian, even before the
/// entity exists); an ambiguous one asks the GM to narrow it. `kind` only labels the
/// error message.
fn resolve_link_name(
    entries: &[(String, String)],
    trimmed: &str,
    kind: &str,
) -> Result<String, String> {
    match match_entity(entries, trimmed) {
        EntityMatch::Found(name, _) => Ok(name),
        EntityMatch::None => Ok(trimmed.to_string()),
        EntityMatch::Ambiguous => Err(format!(
            "Several {kind} match \"{trimmed}\" — pick one from the list or keep typing."
        )),
    }
}

/// Append a link name to a list, deduping case-insensitively (the repeatable
/// allies/rivals pickers can re-enter the same name). Returns whether it was added.
fn add_link(list: &mut Vec<String>, name: &str) -> bool {
    if list.iter().any(|existing| existing.eq_ignore_ascii_case(name)) {
        return false;
    }
    list.push(name.to_string());
    true
}

/// The Great-House brand menu labels, derived from the canonical `HOUSE_BRANDS`
/// tokens so the menu can't drift from the vocab. The stored brand is the readable
/// phrase (or custom free text).
fn brand_labels() -> Vec<&'static str> {
    HOUSE_BRANDS.iter().map(|token| brand_phrase(token)).collect()
}

/// A readable phrase for a `HOUSE_BRANDS` token (the menu label + stored value).
fn brand_phrase(token: &str) -> &'static str {
    match token {
        "wealth" => "wealth",
        "loyalty" => "loyalty",
        "martial" => "martial might",
        "piety" => "piety",
        "cunning" => "cunning",
        "lineage" => "ancient lineage",
        _ => "its standing",
    }
}

/// Pick a loyalty type at random (option `0`); always resolves to a value (design §6).
/// Seeded off the wall clock to avoid a new dependency — randomness need not be strong.
fn random_loyalty() -> &'static str {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    LOYALTY_TYPES[(nanos as usize) % LOYALTY_TYPES.len()]
}

/// Load the linkable factions (read-only) into the accumulator and jump to the shared
/// faction picker in the given mode (liege / patron / allies / rivals).
async fn enter_faction_pick(
    d: &mut WizardData,
    state: &AppState,
    mode: &'static str,
) -> Result<WizardTransition, String> {
    let factions = load_linkable_factions(state).await?;
    let data = faction_data_mut(d);
    data.factions = factions;
    data.link_return = Some(mode);
    Ok(WizardTransition::Goto("faction_pick"))
}

/// Load the linkable NPCs (read-only) into the accumulator and jump to the leader picker.
async fn enter_npc_pick(d: &mut WizardData, state: &AppState) -> Result<WizardTransition, String> {
    let npcs = load_linkable_npcs(state).await?;
    faction_data_mut(d).npcs = npcs;
    Ok(WizardTransition::Goto("npc_pick"))
}

/// Load the linkable gods (read-only) into the accumulator and jump to the god picker.
async fn enter_god_pick(d: &mut WizardData, state: &AppState) -> Result<WizardTransition, String> {
    let gods = load_linkable_gods(state).await?;
    faction_data_mut(d).gods = gods;
    Ok(WizardTransition::Goto("god_pick"))
}

/// Run wizard generation into the accumulator. Factions are always structured (there
/// is no one-shot lane in the wizard), so this always goes through
/// `generate_faction_seed_for_wizard`.
async fn generate_faction_into(d: &mut FactionWizardData, state: &AppState) -> Result<(), String> {
    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let SeedGeneration { seed, notice } = ai
        .generate_faction_seed_for_wizard(
            &d.as_inputs(),
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;
    d.seed = Some(seed);
    d.notice = notice;
    Ok(())
}

/// Flatten the locked answers into a single bias string so a later `faction reroll
/// <field>` reuses the GM's intent (the reroll service merges `seed_prompt`).
fn build_seed_prompt(d: &FactionWizardData) -> Option<String> {
    Some(build_faction_wizard_user_prompt(&d.as_inputs()))
}

/// Build the editable `FactionDraft` from the accumulator's generated seed + locked
/// answers. `kind_type` comes from the accumulator (GM-locked at the category/kind
/// steps), never the model; the relational fields (leader/allies/rivals/liege/
/// loyalty) come from the pickers, never the seed (D3). Returns `None` if generation
/// never produced a seed.
fn build_faction_draft(d: &FactionWizardData, id: String) -> Option<FactionDraftSession> {
    let seed = d.seed.clone()?;
    Some(FactionDraftSession {
        id,
        seed_prompt: build_seed_prompt(d),
        slug: slugify(&seed.name),
        name: seed.name,
        vault_path: String::new(),
        // GM-locked at the category/kind steps — never the model's pick.
        kind_type: d.kind_type.clone(),
        public_description: seed.public_description,
        reputation: seed.reputation,
        symbol_description: seed.symbol_description,
        want: seed.want,
        obstacle: seed.obstacle,
        action: seed.action,
        consequence: seed.consequence,
        sphere_of_influence: seed.sphere_of_influence,
        resources_assets: seed.resources_assets,
        // Relational/place fields from the pickers (or blank), never the seed (D3/§7).
        leader: d.leader.clone().unwrap_or_default(),
        allies: d.allies.clone(),
        rivals_enemies: d.rivals.clone(),
        liege: d.liege.clone(),
        loyalty_type: d.loyalty_type.clone(),
        // Wizard-built: request category-based subfoldering of the published `.md` path.
        wizard_subfoldered: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use runebound_models::utils::FACTION_KIND_TYPES;

    fn houses_seed() -> FactionSeed {
        FactionSeed {
            name: "House Corvane".to_string(),
            // The model's pick — the wizard must ignore it and lock from the kind step.
            kind_type: "cult".to_string(),
            public_description: "An old salt-house.".to_string(),
            reputation: "Respected, quietly feared.".to_string(),
            symbol_description: "A black salt-crystal on grey.".to_string(),
            want: "Control every grain of salt in the basin.".to_string(),
            obstacle: "The vein is running dry.".to_string(),
            action: "Buying up rival pans before the news spreads.".to_string(),
            consequence: "If they fail, the preservation trade collapses.".to_string(),
            sphere_of_influence: "The coastal salt flats.".to_string(),
            resources_assets: vec!["salt pans".to_string(), "brine wells".to_string()],
        }
    }

    #[test]
    fn draft_locks_kind_from_accumulator_and_sets_subfoldered() {
        let mut d = FactionWizardData {
            kind_type: "major_vassal".to_string(),
            ..Default::default()
        };
        d.seed = Some(houses_seed());
        let draft = build_faction_draft(&d, "fac_test".to_string()).expect("draft");
        // kind comes from the kind step, never the model's pick.
        assert_eq!(draft.kind_type, "major_vassal");
        assert_eq!(draft.slug, "house-corvane");
        // The wizard path requests category-based subfoldering of the published `.md`.
        assert!(draft.wizard_subfoldered);
    }

    #[test]
    fn draft_fills_relational_from_accumulator_not_seed() {
        let mut d = FactionWizardData {
            kind_type: "individual_lord".to_string(),
            leader: Some("Ser Aldric".to_string()),
            allies: vec!["House Vey".to_string()],
            rivals: vec!["The Dust Choir".to_string()],
            liege: Some("House Vaurel".to_string()),
            loyalty_type: Some("oath".to_string()),
            ..Default::default()
        };
        d.seed = Some(houses_seed());
        let draft = build_faction_draft(&d, "fac_rel".to_string()).expect("draft");
        // Relational fields come from the pickers, never the LLM seed (D3).
        assert_eq!(draft.leader, "Ser Aldric");
        assert_eq!(draft.allies, vec!["House Vey".to_string()]);
        assert_eq!(draft.rivals_enemies, vec!["The Dust Choir".to_string()]);
        assert_eq!(draft.liege.as_deref(), Some("House Vaurel"));
        assert_eq!(draft.loyalty_type.as_deref(), Some("oath"));
    }

    #[test]
    fn draft_leaves_relational_blank_when_unset() {
        let mut d = FactionWizardData {
            kind_type: "guild".to_string(),
            ..Default::default()
        };
        d.seed = Some(houses_seed());
        let draft = build_faction_draft(&d, "fac_blank".to_string()).expect("draft");
        assert_eq!(draft.leader, "");
        assert!(draft.allies.is_empty());
        assert!(draft.rivals_enemies.is_empty());
        assert_eq!(draft.liege, None);
        assert_eq!(draft.loyalty_type, None);
    }

    #[test]
    fn power_specifics_routes_great_house_to_brand_others_to_liege() {
        assert!(power_specifics_next_is_brand("great_house"));
        for vassal in ["major_vassal", "minor_vassal", "individual_lord"] {
            assert!(!power_specifics_next_is_brand(vassal));
        }
    }

    #[test]
    fn menu_kind_values_are_all_canonical() {
        for kind in HOUSE_LAYER_VALUES
            .iter()
            .chain(EST_KIND_VALUES.iter())
            .chain(REL_KIND_VALUES.iter())
        {
            assert!(
                FACTION_KIND_TYPES.contains(kind),
                "{kind} must be one of the 9 canonical kinds"
            );
        }
        // The three branch menus together cover all 9 kinds exactly once.
        let menu_count =
            HOUSE_LAYER_VALUES.len() + EST_KIND_VALUES.len() + REL_KIND_VALUES.len();
        assert_eq!(menu_count, FACTION_KIND_TYPES.len());
    }

    #[test]
    fn pick_value_maps_one_based_index() {
        assert_eq!(pick_value("1", &LORD_TYPES), Some("chokepoint"));
        assert_eq!(pick_value("6", &LORD_TYPES), Some("extraction"));
        assert_eq!(pick_value("0", &LORD_TYPES), None);
        assert_eq!(pick_value("7", &LORD_TYPES), None);
        assert_eq!(pick_value("x", &LORD_TYPES), None);
    }

    #[test]
    fn brand_labels_cover_all_house_brands() {
        assert_eq!(brand_labels().len(), HOUSE_BRANDS.len());
        // "martial" reads as "martial might" so the menu phrase is natural.
        assert_eq!(brand_phrase("martial"), "martial might");
    }

    #[test]
    fn random_loyalty_is_a_valid_type() {
        assert!(LOYALTY_TYPES.contains(&random_loyalty()));
    }

    #[test]
    fn add_link_dedupes_case_insensitively() {
        let mut list = Vec::new();
        assert!(add_link(&mut list, "House Vey"));
        // Same name (any case) is not re-added.
        assert!(!add_link(&mut list, "house vey"));
        assert!(add_link(&mut list, "The Dust Choir"));
        assert_eq!(list, vec!["House Vey".to_string(), "The Dust Choir".to_string()]);
    }

    fn pick_data(mode: &'static str, factions: &[(&str, &str)]) -> WizardData {
        WizardData::new(FactionWizardData {
            link_return: Some(mode),
            factions: factions
                .iter()
                .map(|(name, slug)| (name.to_string(), slug.to_string()))
                .collect(),
            ..Default::default()
        })
    }

    #[test]
    fn liege_mode_is_mandatory_no_skip() {
        // The liege link is required, so neither the choices nor the typeahead expose
        // a `skip` or `done`.
        let data = pick_data("liege", &[("House Vaurel", "house-vaurel")]);
        assert!(FactionPickStep.choices(&data).is_empty());
        let tokens: Vec<String> = FactionPickStep
            .suggest("", &data)
            .into_iter()
            .map(|choice| choice.token)
            .collect();
        assert!(tokens.contains(&"House Vaurel".to_string()));
        assert!(!tokens.contains(&"skip".to_string()));
        assert!(!tokens.contains(&"done".to_string()));
    }

    #[test]
    fn patron_mode_offers_skip_and_typeahead() {
        let data = pick_data("patron", &[("House Vaurel", "house-vaurel")]);
        let choice_tokens: Vec<String> = FactionPickStep
            .choices(&data)
            .into_iter()
            .map(|choice| choice.token)
            .collect();
        assert!(choice_tokens.contains(&"skip".to_string()));
        let suggest_tokens: Vec<String> = FactionPickStep
            .suggest("", &data)
            .into_iter()
            .map(|choice| choice.token)
            .collect();
        assert!(suggest_tokens.contains(&"House Vaurel".to_string()));
        assert!(suggest_tokens.contains(&"skip".to_string()));
    }

    #[test]
    fn allies_mode_offers_done_not_skip() {
        let data = pick_data("allies", &[("House Vey", "house-vey")]);
        let choice_tokens: Vec<String> = FactionPickStep
            .choices(&data)
            .into_iter()
            .map(|choice| choice.token)
            .collect();
        assert!(choice_tokens.contains(&"done".to_string()));
        let suggest_tokens: Vec<String> = FactionPickStep
            .suggest("", &data)
            .into_iter()
            .map(|choice| choice.token)
            .collect();
        assert!(suggest_tokens.contains(&"House Vey".to_string()));
        assert!(suggest_tokens.contains(&"done".to_string()));
    }

    #[test]
    fn seed_prompt_carries_locked_answers() {
        let d = FactionWizardData {
            kind_type: "major_vassal".to_string(),
            power_base: Some("extraction".to_string()),
            liege: Some("House Vaurel".to_string()),
            loyalty_type: Some("oath".to_string()),
            want: Some("Corner the salt trade".to_string()),
            ..Default::default()
        };
        let prompt = build_seed_prompt(&d).expect("seed prompt");
        assert!(prompt.contains("major_vassal"));
        assert!(prompt.contains("extraction"));
        // The liege is emitted as an `@factions/` probe so its metadata is pulled in.
        assert!(prompt.contains("@factions/House Vaurel"));
        assert!(prompt.contains("Corner the salt trade"));
    }
}
