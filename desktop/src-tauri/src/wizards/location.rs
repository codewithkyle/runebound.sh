//! The location wizard: the guided `create location` flow expressed as declarative
//! `WizardStep`s on the shared engine (docs/architecture.md §4). Step 1 picks the
//! GM-locked `kind_type`, which routes to one of five branches — Settlement, Site,
//! Hideout, Guildhall (a faction's public HQ, so it opens on a mandatory faction
//! link), or the minimal/custom lane — each of which ends by generating the
//! LLM-derived fields *under* the GM's locked answers and converging on the same
//! `LocationDraft` the one-shot `create_location` produced. So `save`/`reroll`/the
//! card UI all keep working unchanged (the dungeon model applied to location).
//!
//! The locked answers live only in this accumulator; they are baked into the
//! generated prose and the derived/locked fields, and flattened into `seed_prompt`
//! as reroll bias. Nothing new is persisted (no draft/row/migration changes).

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use dnd_core::npc::slugify;
use runebound_models::output::{
    OutputDoc, command_ref, doc, heading, paragraph_text, paragraph_with_inlines, text_node,
};
use runebound_models::utils::{LOCATION_DANGER_LEVELS, LOCATION_KIND_TYPES, make_entity_id};

use crate::app_state::{AppState, LocationDraftSession};
use crate::commands::{location_event_from_draft, location_summary_text};
use crate::entities::common::{
    CommandResult, command_message_response_with_doc, command_response_with_event,
};
use crate::services::ai_generation::{
    AiGenerationService, LocationSeed, LocationWizardInputs, SeedGeneration,
    build_wizard_user_prompt, location_subfolder,
};
use crate::utils::prepend_notice;
use crate::wizards::entity_link::{
    EntityMatch, entity_suggestions, entries_from_refs, load_linkable_factions,
    load_vault_entries_blocking, match_entity,
};

use wizard::prompt::wizard_menu;
use wizard::{Wizard, WizardChoice, WizardData, WizardStep, WizardTransition};

// ---------------------------------------------------------------------------
// Accumulator
// ---------------------------------------------------------------------------

/// A location the GM can anchor a guildhall to. Unlike the flat faction picker's
/// `(name, slug)` tuple, this carries the note's under-`locations/` subfolder (`""`
/// when flat) so the chosen anchor's `@locations/<sub>/<anchor>` seed is path-keyed.
#[derive(Debug, Clone, Default)]
struct LocationAnchorChoice {
    name: String,
    slug: String,
    sub: String,
}

/// The per-flow answers; the cursor/history live in the engine's `WizardSession`.
/// `seed`/`notice` carry the generated result and a one-shot capacity notice into
/// the `finalize` hand-off that opens the location editor.
#[derive(Debug, Clone, Default)]
struct LocationWizardData {
    // Step 1
    kind_type: String,
    kind_custom: Option<String>,
    /// Set after option `0`: the next submission on the kind step is the custom name.
    awaiting_custom_kind: bool,

    // Settlement (Q-A…Q-D)
    control: Option<String>,
    resources: Option<String>,
    export_mode: Option<String>,

    // Site (Q-S1…Q-S3)
    site_focus: Option<String>,
    site_draw: Option<String>,

    // Hideout (Q-H1…Q-H4)
    base_owner: Option<String>,
    base_protection: Option<String>,
    base_purpose: Option<String>,

    // Guildhall (faction-locked public HQ): its public-facing function, and the
    // existing location it stands within (or a free-typed place name).
    public_role: Option<String>,
    location_anchor: Option<String>,
    // The chosen anchor's under-`locations/` subfolder (`Some("settlements")`, …), so
    // the `@locations/<sub>/<anchor>` seed reference is path-keyed correctly. `None`
    // for a flat note or a free-typed place (the seed falls back to name-only).
    location_anchor_sub: Option<String>,

    // GM-locked danger for Site + Hideout (Q-S2 / Q-H3)
    danger_lock: Option<String>,

    // Shared optional map anchor (Q-D / Q-S4 / Q-H5)
    geography: Option<String>,

    // Minimal / custom seed (free text)
    custom_seed: Option<String>,

    // Read-only faction link (Q-A / Q-H1): the searchable set loaded on entry (the
    // typeahead filters it in memory), plus the chosen faction's canonical name +
    // slug, and which step requested the link.
    factions: Vec<(String, String)>,
    faction_name: Option<String>,
    faction_ref: Option<String>,
    faction_link_return: Option<&'static str>,

    // Guildhall location anchor (Q-G3): the searchable set of existing locations
    // loaded on entry to the anchor step; the chosen name lands in `location_anchor`
    // and its subfolder in `location_anchor_sub`. Carries the subfolder (unlike the
    // flat faction set) so the path-keyed `@reference` resolves.
    locations: Vec<LocationAnchorChoice>,

    // Generated seed handed to `finalize`/the editor, plus a one-shot notice.
    seed: Option<LocationSeed>,
    notice: Option<String>,
}

fn location_data(d: &WizardData) -> &LocationWizardData {
    d.downcast_ref::<LocationWizardData>()
        .expect("location wizard data")
}

fn location_data_mut(d: &mut WizardData) -> &mut LocationWizardData {
    d.downcast_mut::<LocationWizardData>()
        .expect("location wizard data")
}

impl LocationWizardData {
    /// The structured branches (Settlement/Site/Hideout/Guildhall) generate under
    /// locked answers; only the freeform `other` lane stays on the one-shot path.
    fn is_structured(&self) -> bool {
        is_structured(&self.kind_type)
    }

    fn as_inputs(&self, hint: Option<&str>) -> LocationWizardInputs {
        LocationWizardInputs {
            kind_type: self.kind_type.clone(),
            kind_custom: self.kind_custom.clone(),
            control: self.control.clone(),
            resources: self.resources.clone(),
            export_mode: self.export_mode.clone(),
            site_focus: self.site_focus.clone(),
            site_draw: self.site_draw.clone(),
            base_owner: self.base_owner.clone(),
            base_protection: self.base_protection.clone(),
            base_purpose: self.base_purpose.clone(),
            public_role: self.public_role.clone(),
            location_anchor: self.location_anchor.clone(),
            location_anchor_sub: self.location_anchor_sub.clone(),
            danger_lock: self.danger_lock.clone(),
            geography: self.geography.clone(),
            faction_name: self.faction_name.clone(),
            hint: hint.map(str::to_string),
        }
    }

    /// The one-shot prompt for the freeform custom-kind lane: the kind framing plus
    /// the GM's free-text seed and any reroll hint. (Guildhall is structured now, so
    /// only `other` reaches this.)
    fn custom_prompt(&self, hint: Option<&str>) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if self.kind_type == "other"
            && let Some(custom) = trimmed_opt(&self.kind_custom)
        {
            parts.push(format!("A {custom}."));
        }
        if let Some(seed) = trimmed_opt(&self.custom_seed) {
            parts.push(seed.to_string());
        }
        if let Some(hint) = hint.map(str::trim).filter(|h| !h.is_empty()) {
            parts.push(hint.to_string());
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    }
}

fn is_structured(kind_type: &str) -> bool {
    matches!(
        kind_type,
        "hamlet" | "town" | "city" | "ruin" | "landmark" | "wilderness" | "hideout" | "guildhall"
    )
}

fn trimmed_opt(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

// ---------------------------------------------------------------------------
// Step 1 — kind (router)
// ---------------------------------------------------------------------------

/// The concrete kinds the picker offers (everything but `other`, which is reached
/// via option `0`). Derived from the canonical list so it can never drift.
fn kind_menu() -> Vec<&'static str> {
    LOCATION_KIND_TYPES
        .iter()
        .copied()
        .filter(|kind| *kind != "other")
        .collect()
}

struct KindStep;

#[async_trait]
impl WizardStep<AppState> for KindStep {
    fn id(&self) -> &'static str {
        "kind"
    }

    fn summary(&self) -> &'static str {
        "Pick what kind of place this is (it routes the rest of the flow), or 0 for a custom kind."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        if location_data(data).awaiting_custom_kind {
            return doc()
                .with_block(heading(2, "Create Location — Custom Kind"))
                .with_block(paragraph_text("Type a name for this custom kind."));
        }
        wizard_menu(
            "Create Location — Kind",
            "What kind of place is this?",
            &self.choices(data),
        )
    }

    fn choices(&self, data: &WizardData) -> Vec<WizardChoice> {
        if location_data(data).awaiting_custom_kind {
            return Vec::new();
        }
        let mut choices = vec![
            WizardChoice::new("0: custom / freeform", "0")
                .with_help("Name your own kind, then describe it"),
        ];
        for (i, kind) in kind_menu().into_iter().enumerate() {
            choices.push(WizardChoice::new(
                format!("{}: {kind}", i + 1),
                (i + 1).to_string(),
            ));
        }
        choices
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();

        // Second phase of option 0: capture the custom kind name, then generate.
        {
            let data = location_data_mut(d);
            if data.awaiting_custom_kind {
                if trimmed.is_empty() {
                    return Ok(WizardTransition::Stay);
                }
                data.kind_custom = Some(trimmed.to_string());
                data.kind_type = "other".to_string();
                data.awaiting_custom_kind = false;
                return Ok(WizardTransition::Goto("custom_seed"));
            }

            if trimmed == "0" {
                data.awaiting_custom_kind = true;
                return Ok(WizardTransition::Stay);
            }
        }

        let menu = kind_menu();
        let Ok(n) = trimmed.parse::<usize>() else {
            return Ok(WizardTransition::Stay);
        };
        if !(1..=menu.len()).contains(&n) {
            return Ok(WizardTransition::Stay);
        }
        let kind = menu[n - 1];
        location_data_mut(d).kind_type = kind.to_string();
        match kind {
            "hamlet" | "town" | "city" => Ok(WizardTransition::Goto("control")),
            "ruin" | "landmark" | "wilderness" => Ok(WizardTransition::Goto("site_focus")),
            "hideout" => Ok(WizardTransition::Goto("base_owner")),
            // A guildhall is a faction's public HQ, so it opens straight on the
            // (mandatory) faction link rather than the minimal lane.
            "guildhall" => enter_faction_link(d, state, "guildhall").await,
            _ => Ok(WizardTransition::Stay),
        }
    }
}

// ---------------------------------------------------------------------------
// Settlement branch (Q-A…Q-D)
// ---------------------------------------------------------------------------

const CONTROL_LABELS: [&str; 4] = [
    "noble house / lord",
    "faction, council, or guild",
    "free city",
    "independent / contested",
];
/// Parallel to `CONTROL_LABELS`; the authoritative phrasing fed to generation.
/// Every archetype is concrete — a settlement must have a stated power, so there is
/// no "let the model decide" escape hatch here.
const CONTROL_VALUES: [&str; 4] = [
    "a noble house or lord",
    "a faction, council, or guild",
    "a free city",
    "independent or contested rule",
];

struct ControlStep;

#[async_trait]
impl WizardStep<AppState> for ControlStep {
    fn id(&self) -> &'static str {
        "control"
    }

    fn summary(&self) -> &'static str {
        "Who controls this settlement? A house or a faction/guild links a specific faction."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Location — Settlement — Control",
            "Who controls it?",
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
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        let Some(value) = pick_value(trimmed, &CONTROL_VALUES) else {
            return Ok(WizardTransition::Stay);
        };
        let data = location_data_mut(d);
        // Re-answering control clears any prior faction link.
        data.faction_name = None;
        data.faction_ref = None;
        data.control = Some(value.to_string());
        // A noble house/lord or a faction/council/guild is a concrete organization, so
        // let the GM link the specific one — grounding the prose — falling back to this
        // archetype on skip. A free city or contested rule has no single controlling
        // organization to link.
        if value == CONTROL_VALUES[0] || value == CONTROL_VALUES[1] {
            return enter_faction_link(d, state, "control").await;
        }
        Ok(WizardTransition::Next)
    }
}

struct ResourcesStep;

#[async_trait]
impl WizardStep<AppState> for ResourcesStep {
    fn id(&self) -> &'static str {
        "resources"
    }

    fn summary(&self) -> &'static str {
        "Free text: what natural resources are here (the GM's map knowledge)."
    }

    fn prompt(&self, _data: &WizardData) -> OutputDoc {
        doc()
            .with_block(heading(2, "Create Location — Settlement — Resources"))
            .with_block(paragraph_text("What natural resources are here?"))
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(WizardTransition::Stay);
        }
        location_data_mut(d).resources = Some(trimmed.to_string());
        Ok(WizardTransition::Next)
    }
}

const EXPORT_MODE_LABELS: [&str; 4] = ["raw", "refined", "mixed", "none"];

struct ExportModeStep;

#[async_trait]
impl WizardStep<AppState> for ExportModeStep {
    fn id(&self) -> &'static str {
        "export_mode"
    }

    fn summary(&self) -> &'static str {
        "What does it export — raw, refined, mixed, or none? (transport logistics)"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Location — Settlement — Exports",
            "What does it export? (a frontier town may export nothing)",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&EXPORT_MODE_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(value) = pick_value(input.trim(), &EXPORT_MODE_LABELS) else {
            return Ok(WizardTransition::Stay);
        };
        location_data_mut(d).export_mode = Some(value.to_string());
        Ok(WizardTransition::Next)
    }
}

// ---------------------------------------------------------------------------
// Site branch (Q-S1…Q-S4)
// ---------------------------------------------------------------------------

const SITE_FOCUS_LABELS: [&str; 3] = ["what it was", "what's here now", "balanced"];
const SITE_FOCUS_VALUES: [&str; 3] = ["past", "present", "balanced"];

struct SiteFocusStep;

#[async_trait]
impl WizardStep<AppState> for SiteFocusStep {
    fn id(&self) -> &'static str {
        "site_focus"
    }

    fn summary(&self) -> &'static str {
        "Is this place about what it WAS, or what's HERE NOW?"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Location — Site — Focus",
            "Is this place about what it was, or what's here now?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&SITE_FOCUS_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(value) = pick_value(input.trim(), &SITE_FOCUS_VALUES) else {
            return Ok(WizardTransition::Stay);
        };
        location_data_mut(d).site_focus = Some(value.to_string());
        Ok(WizardTransition::Next)
    }
}

struct SiteDangerStep;

#[async_trait]
impl WizardStep<AppState> for SiteDangerStep {
    fn id(&self) -> &'static str {
        "site_danger"
    }

    fn summary(&self) -> &'static str {
        "Pick the danger level (you own it for a site); the model writes the source."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Location — Site — Danger",
            "How dangerous is this place? (the model writes why)",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        danger_choices()
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(value) = pick_danger(input.trim()) else {
            return Ok(WizardTransition::Stay);
        };
        location_data_mut(d).danger_lock = Some(value);
        Ok(WizardTransition::Next)
    }
}

const SITE_DRAW_LABELS: [&str; 5] = [
    "loot",
    "quest objective",
    "passage / shortcut",
    "a person who lives here",
    "buried lore",
];
const SITE_DRAW_VALUES: [&str; 5] = [
    "loot waiting to be claimed",
    "a quest objective",
    "a passage or shortcut to somewhere else",
    "a person who lives here",
    "buried lore",
];

struct SiteDrawStep;

#[async_trait]
impl WizardStep<AppState> for SiteDrawStep {
    fn id(&self) -> &'static str {
        "site_draw"
    }

    fn summary(&self) -> &'static str {
        "Optional: why do players come here? Pick one, type your own, or skip (the model picks)."
    }

    fn prompt(&self, _data: &WizardData) -> OutputDoc {
        let mut document = doc()
            .with_block(heading(2, "Create Location — Site — The Draw"))
            .with_block(paragraph_with_inlines(vec![
                text_node("Why do players come here? Pick one below, type your own, or "),
                command_ref("skip", "skip"),
                text_node(" to let the model decide."),
            ]));
        document = document.with_block(wizard::prompt::choice_lines(&numbered_choices(
            &SITE_DRAW_LABELS,
        )));
        document
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        let mut choices = numbered_choices(&SITE_DRAW_LABELS);
        choices.push(WizardChoice::new("skip", "skip").with_help("Let the model pick the draw"));
        choices
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        let value = if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("skip") {
            None
        } else if let Some(value) = pick_value(trimmed, &SITE_DRAW_VALUES) {
            Some(value.to_string())
        } else {
            Some(trimmed.to_string())
        };
        location_data_mut(d).site_draw = value;
        Ok(WizardTransition::Next)
    }
}

// ---------------------------------------------------------------------------
// Hideout branch (Q-H1…Q-H5)
// ---------------------------------------------------------------------------

const BASE_OWNER_LABELS: [&str; 4] = [
    "faction / guild",
    "a single operator",
    "a creature / monster",
    "a cult",
];
const BASE_OWNER_VALUES: [&str; 4] = [
    "a faction or guild",
    "a single operator (a chief, hermit, or rogue mage)",
    "a creature or monster",
    "a cult",
];

struct BaseOwnerStep;

#[async_trait]
impl WizardStep<AppState> for BaseOwnerStep {
    fn id(&self) -> &'static str {
        "base_owner"
    }

    fn summary(&self) -> &'static str {
        "Whose base is it? A faction/guild links a specific faction."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Location — Hideout — Owner",
            "Whose base is it?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&BASE_OWNER_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        let Some(value) = pick_value(trimmed, &BASE_OWNER_VALUES) else {
            return Ok(WizardTransition::Stay);
        };
        let data = location_data_mut(d);
        // Re-answering ownership clears any prior faction link.
        data.faction_name = None;
        data.faction_ref = None;
        data.base_owner = Some(value.to_string());
        // A faction or guild is a concrete organization, so let the GM link the specific
        // one — grounding the prose — falling back to this archetype on skip. The other
        // owners (a lone operator, a creature, a cult) need no link.
        if value == BASE_OWNER_VALUES[0] {
            return enter_faction_link(d, state, "base_owner").await;
        }
        Ok(WizardTransition::Next)
    }
}

const BASE_PROTECTION_LABELS: [&str; 4] = ["secrecy", "force", "both", "barely"];

struct BaseProtectionStep;

#[async_trait]
impl WizardStep<AppState> for BaseProtectionStep {
    fn id(&self) -> &'static str {
        "base_protection"
    }

    fn summary(&self) -> &'static str {
        "Protected by what? (covers both how hidden and how fortified)"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Location — Hideout — Protection",
            "Protected by what?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&BASE_PROTECTION_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(value) = pick_value(input.trim(), &BASE_PROTECTION_LABELS) else {
            return Ok(WizardTransition::Stay);
        };
        location_data_mut(d).base_protection = Some(value.to_string());
        Ok(WizardTransition::Next)
    }
}

struct BaseDangerStep;

#[async_trait]
impl WizardStep<AppState> for BaseDangerStep {
    fn id(&self) -> &'static str {
        "base_danger"
    }

    fn summary(&self) -> &'static str {
        "Pick the danger level (you own it for a base); the model writes the source."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Location — Hideout — Danger",
            "How dangerous is this base? (the model writes why)",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        danger_choices()
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(value) = pick_danger(input.trim()) else {
            return Ok(WizardTransition::Stay);
        };
        location_data_mut(d).danger_lock = Some(value);
        Ok(WizardTransition::Next)
    }
}

const BASE_PURPOSE_LABELS: [&str; 5] = ["raids", "smuggling", "refuge", "ritual", "vault"];

struct BasePurposeStep;

#[async_trait]
impl WizardStep<AppState> for BasePurposeStep {
    fn id(&self) -> &'static str {
        "base_purpose"
    }

    fn summary(&self) -> &'static str {
        "What is the base for? (this is plot, not flavor)"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Location — Hideout — Purpose",
            "What is the base for?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        numbered_choices(&BASE_PURPOSE_LABELS)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let Some(value) = pick_value(input.trim(), &BASE_PURPOSE_LABELS) else {
            return Ok(WizardTransition::Stay);
        };
        location_data_mut(d).base_purpose = Some(value.to_string());
        Ok(WizardTransition::Next)
    }
}

// ---------------------------------------------------------------------------
// Guildhall branch — faction-locked public HQ (faction link runs first)
// ---------------------------------------------------------------------------

const GUILDHALL_ROLE_LABELS: [&str; 5] = [
    "counting house / bank",
    "training hall",
    "trade exchange",
    "lodge / chapterhouse",
    "courthouse / tribunal",
];
const GUILDHALL_ROLE_VALUES: [&str; 5] = [
    "a counting house or bank",
    "a training hall",
    "a trade exchange or market hall",
    "a lodge or chapterhouse",
    "a courthouse or tribunal",
];

struct GuildhallRoleStep;

#[async_trait]
impl WizardStep<AppState> for GuildhallRoleStep {
    fn id(&self) -> &'static str {
        "guildhall_role"
    }

    fn summary(&self) -> &'static str {
        "Optional: the hall's public function. Pick one, type your own, or skip (the model picks)."
    }

    fn prompt(&self, _data: &WizardData) -> OutputDoc {
        let mut document = doc()
            .with_block(heading(2, "Create Location — Guildhall — Public Role"))
            .with_block(paragraph_with_inlines(vec![
                text_node("What is this hall's public face? Pick one below, type your own, or "),
                command_ref("skip", "skip"),
                text_node(" to let the model decide."),
            ]));
        document = document.with_block(wizard::prompt::choice_lines(&numbered_choices(
            &GUILDHALL_ROLE_LABELS,
        )));
        document
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        let mut choices = numbered_choices(&GUILDHALL_ROLE_LABELS);
        choices.push(
            WizardChoice::new("skip", "skip").with_help("Let the model pick the hall's role"),
        );
        choices
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        let value = if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("skip") {
            None
        } else if let Some(value) = pick_value(trimmed, &GUILDHALL_ROLE_VALUES) {
            Some(value.to_string())
        } else {
            Some(trimmed.to_string())
        };
        location_data_mut(d).public_role = value;
        // Load the existing locations for the (mandatory) anchor picker, then enter it.
        enter_location_anchor(d, state).await
    }
}

/// The guildhall's terminal step: pick the existing location the hall stands within
/// (typeahead), or name a new place. Required — a hall stands *somewhere* — but a
/// free-typed name is accepted so the GM is never blocked. Generates on a valid
/// answer, then completes (hands off to the editor like every other branch).
struct GuildhallAnchorStep;

#[async_trait]
impl WizardStep<AppState> for GuildhallAnchorStep {
    fn id(&self) -> &'static str {
        "guildhall_anchor"
    }

    fn summary(&self) -> &'static str {
        "Pick the location this hall stands in (type to search), or name a new place."
    }

    fn awaiting_llm_label(&self) -> Option<&'static str> {
        Some("generating location")
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        let d = location_data(data);
        let body = if d.locations.is_empty() {
            "Where does this hall stand? No locations exist yet — type the name of the place that contains it."
        } else {
            "Where does this hall stand? Start typing to select one of your locations, or name a new place."
        };
        doc()
            .with_block(heading(2, "Create Location — Guildhall — Where It Stands"))
            .with_block(paragraph_text(body))
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        // Mandatory and typeahead-driven, so no enumerated choices (and no `skip`).
        Vec::new()
    }

    fn suggest(&self, input: &str, data: &WizardData) -> Vec<WizardChoice> {
        // Project to `(name, slug)` for the shared, faction-flat typeahead helper.
        entity_suggestions(&anchor_pairs(&location_data(data).locations), input)
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            // Required: re-prompt rather than generating an unanchored hall.
            return Ok(WizardTransition::Stay);
        }
        let (anchor, sub) = resolve_anchor(&location_data(d).locations, trimmed)?;
        let data = location_data_mut(d);
        data.location_anchor = Some(anchor);
        data.location_anchor_sub = sub;
        generate_location_into(location_data_mut(d), state, None).await?;
        Ok(WizardTransition::Complete)
    }
}

/// Project the location anchor choices down to the flat `(name, slug)` tuples the
/// shared typeahead/match helpers (also used by the faction picker) consume.
fn anchor_pairs(locations: &[LocationAnchorChoice]) -> Vec<(String, String)> {
    locations
        .iter()
        .map(|choice| (choice.name.clone(), choice.slug.clone()))
        .collect()
}

/// Resolve typed anchor input against the loaded locations: the anchor display name
/// plus the subfolder to thread into the `@locations/<sub>/<anchor>` seed (`None` for
/// a flat note or a free-typed place). `Err` on an ambiguous match. Pure, so the
/// subfolder capture is unit-testable without the LLM round-trip that follows it.
fn resolve_anchor(
    locations: &[LocationAnchorChoice],
    trimmed: &str,
) -> Result<(String, Option<String>), String> {
    match match_entity(&anchor_pairs(locations), trimmed) {
        EntityMatch::Found(name, slug) => {
            // Carry the picked note's subfolder; `""` (a flat note) maps to `None`.
            let sub = locations
                .iter()
                .find(|choice| choice.slug == slug)
                .map(|choice| choice.sub.clone())
                .filter(|sub| !sub.is_empty());
            Ok((name, sub))
        }
        EntityMatch::Ambiguous => Err(format!(
            "Several locations match \"{trimmed}\" — pick one from the list or keep typing."
        )),
        // No match: accept the typed text as a new place name (no subfolder).
        EntityMatch::None => Ok((trimmed.to_string(), None)),
    }
}

// ---------------------------------------------------------------------------
// Generate steps — each branch's final (optional) step + the custom seed
// ---------------------------------------------------------------------------

/// Which accumulator field a [`GenerateStep`] records before generating.
#[derive(Clone, Copy)]
enum SeedField {
    Geography,
    CustomSeed,
}

/// The terminal step of every branch: record the (optional, skippable) free text,
/// run generation under the locked answers, then complete the wizard — `finalize`
/// opens the draft straight in the location editor, where save/publish/reroll/cancel
/// live (mirroring the one-shot `create_location`; no separate review screen).
struct GenerateStep {
    id: &'static str,
    title: &'static str,
    body: &'static str,
    field: SeedField,
}

#[async_trait]
impl WizardStep<AppState> for GenerateStep {
    fn id(&self) -> &'static str {
        self.id
    }

    fn summary(&self) -> &'static str {
        "Optional free text, then generate. Type a detail or skip to generate now."
    }

    fn awaiting_llm_label(&self) -> Option<&'static str> {
        Some("generating location")
    }

    fn prompt(&self, _data: &WizardData) -> OutputDoc {
        doc()
            .with_block(heading(2, self.title))
            .with_block(paragraph_with_inlines(vec![
                text_node(self.body),
                text_node(" Or "),
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
        let trimmed = input.trim();
        let value = if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("skip") {
            None
        } else {
            Some(trimmed.to_string())
        };
        {
            let data = location_data_mut(d);
            match self.field {
                SeedField::Geography => data.geography = value,
                SeedField::CustomSeed => data.custom_seed = value,
            }
        }
        generate_location_into(location_data_mut(d), state, None).await?;
        Ok(WizardTransition::Complete)
    }
}

// ---------------------------------------------------------------------------
// Faction link — shared, read-only (Q-A / Q-H1)
// ---------------------------------------------------------------------------

/// Whether the faction link is in its mandatory guildhall mode. A guildhall is a
/// faction's public HQ, so the link is required (no `skip`) and a name that matches
/// nothing is accepted as a brand-new faction reference rather than rejected.
fn faction_link_is_guildhall(data: &LocationWizardData) -> bool {
    data.faction_link_return == Some("guildhall")
}

struct FactionLinkStep;

#[async_trait]
impl WizardStep<AppState> for FactionLinkStep {
    fn id(&self) -> &'static str {
        "faction_link"
    }

    fn summary(&self) -> &'static str {
        "Type to search your factions by name to link a specific one, or skip to let the model invent it."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        let d = location_data(data);
        let mut document = doc().with_block(heading(2, "Create Location — Link a Faction"));

        if faction_link_is_guildhall(d) {
            // Mandatory: a guildhall must belong to a faction. An unknown name is
            // accepted as a new ad hoc faction, so there is no `skip`.
            return document.with_block(paragraph_text(
                "A guildhall is the public face of a faction. Start typing to select a faction, or create a new ad hoc faction.",
            ));
        }

        // The GM already chose the archetype (a house, a faction/guild, …); linking just
        // pins it to a specific faction so its metadata grounds the prose. Skipping keeps
        // that archetype and lets the model invent the specifics.
        if d.factions.is_empty() {
            document = document.with_block(paragraph_with_inlines(vec![
                text_node("No factions exist yet. "),
                command_ref("skip", "skip"),
                text_node(" to let the model invent one."),
            ]));
        } else {
            // A long campaign can have hundreds of factions, so search rather than
            // enumerate: typeahead (`suggest`) lists matches as the GM types.
            document = document.with_block(paragraph_with_inlines(vec![
                text_node("Search your factions to link the specific one, or "),
                command_ref("skip", "skip"),
                text_node(" to let the model invent it."),
            ]));
        }
        document
    }

    fn choices(&self, data: &WizardData) -> Vec<WizardChoice> {
        // The faction set can be huge, so it is offered via `suggest` typeahead
        // rather than enumerated here. In guildhall mode the link is mandatory, so
        // there is no `skip`; otherwise `skip` is the always-listed action.
        if faction_link_is_guildhall(location_data(data)) {
            return Vec::new();
        }
        vec![
            WizardChoice::new("skip", "skip")
                .with_help("Don't link a specific faction; let the model invent it"),
        ]
    }

    /// Typeahead over the factions loaded on entry; `accept` resolves the submitted
    /// display name. `skip` stays reachable except in mandatory guildhall mode.
    fn suggest(&self, input: &str, data: &WizardData) -> Vec<WizardChoice> {
        let d = location_data(data);
        let mut out = entity_suggestions(&d.factions, input);
        let query = input.trim().to_ascii_lowercase();
        if !faction_link_is_guildhall(d) && (query.is_empty() || "skip".starts_with(&query)) {
            out.push(WizardChoice::new("skip", "skip"));
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
        let data = location_data_mut(d);
        let return_step = data.faction_link_return.unwrap_or("control");
        let guildhall = return_step == "guildhall";

        // Where the flow continues after the (optional) link. The control/owner step
        // already locked its archetype, so the link only *refines* it — skipping or
        // linking both move forward to the same next step.
        let next = match return_step {
            "base_owner" => "base_protection",
            "guildhall" => "guildhall_role",
            _ => "resources",
        };

        // Skip / empty: a guildhall link is mandatory (re-prompt); otherwise the
        // archetype the GM already picked stands as the control/owner, so continue
        // forward without pinning a specific faction.
        if trimmed.is_empty() || (!guildhall && trimmed.eq_ignore_ascii_case("skip")) {
            return Ok(if guildhall {
                WizardTransition::Stay
            } else {
                WizardTransition::Goto(next)
            });
        }

        // A linked faction carries its slug; an unmatched name has none. A guildhall
        // accepts an unmatched name as a new ad hoc faction; the optional link rejects
        // it so the GM picks a real one or skips.
        let (name, faction_ref): (String, Option<String>) = match match_entity(
            &data.factions,
            trimmed,
        ) {
            EntityMatch::Found(name, slug) => (name, Some(slug)),
            EntityMatch::Ambiguous => {
                return Err(format!(
                    "Several factions match \"{trimmed}\" — pick one from the list or keep typing."
                ));
            }
            EntityMatch::None if guildhall => (trimmed.to_string(), None),
            EntityMatch::None => {
                return Err(format!(
                    "No faction matches \"{trimmed}\". Type to search, or skip."
                ));
            }
        };

        // The linked faction's name overrides the archetype phrasing on the relevant
        // field; the guildhall locks `authority` via `faction_name` and has no separate
        // control/owner field.
        match return_step {
            "base_owner" => data.base_owner = Some(name.clone()),
            "guildhall" => {}
            _ => data.control = Some(name.clone()),
        }
        data.faction_name = Some(name);
        data.faction_ref = faction_ref;
        Ok(WizardTransition::Goto(next))
    }
}

// ---------------------------------------------------------------------------
// Wizard
// ---------------------------------------------------------------------------

pub struct LocationWizard {
    steps: Vec<Arc<dyn WizardStep<AppState>>>,
}

impl LocationWizard {
    pub fn new() -> Self {
        Self {
            steps: vec![
                Arc::new(KindStep),
                // Settlement
                Arc::new(ControlStep),
                Arc::new(ResourcesStep),
                Arc::new(ExportModeStep),
                Arc::new(GenerateStep {
                    id: "geography_settlement",
                    title: "Create Location — Settlement — Geography",
                    body: "Geography / trade route?",
                    field: SeedField::Geography,
                }),
                // Site
                Arc::new(SiteFocusStep),
                Arc::new(SiteDangerStep),
                Arc::new(SiteDrawStep),
                Arc::new(GenerateStep {
                    id: "geography_site",
                    title: "Create Location — Site — Map Anchor",
                    body: "Geography / map anchor?",
                    field: SeedField::Geography,
                }),
                // Hideout
                Arc::new(BaseOwnerStep),
                Arc::new(BaseProtectionStep),
                Arc::new(BaseDangerStep),
                Arc::new(BasePurposeStep),
                Arc::new(GenerateStep {
                    id: "geography_hideout",
                    title: "Create Location — Hideout — Map Anchor",
                    body: "Where does the base hide?",
                    field: SeedField::Geography,
                }),
                // Guildhall (faction link runs first, from KindStep; anchor generates)
                Arc::new(GuildhallRoleStep),
                Arc::new(GuildhallAnchorStep),
                // Minimal / custom
                Arc::new(GenerateStep {
                    id: "custom_seed",
                    title: "Create Location — Describe It",
                    body: "Describe this place in a sentence or two.",
                    field: SeedField::CustomSeed,
                }),
                // Shared read-only faction link (reached from control / base_owner)
                Arc::new(FactionLinkStep),
            ],
        }
    }
}

#[async_trait]
impl Wizard<AppState> for LocationWizard {
    fn id(&self) -> &'static str {
        "location"
    }

    fn title(&self) -> &'static str {
        "Create Location"
    }

    fn steps(&self) -> &[Arc<dyn WizardStep<AppState>>] {
        &self.steps
    }

    async fn seed(&self, _host: &AppState) -> Result<WizardData, String> {
        Ok(WizardData::new(LocationWizardData::default()))
    }

    /// Runs when a branch's generate step completes: build the `LocationDraft` from
    /// the generated seed + locked answers and open it in the location editor — the
    /// same hand-off the one-shot `create_location` performs. No LLM call here.
    async fn finalize(&self, state: &AppState, d: &WizardData) -> CommandResult {
        let data = location_data(d);
        let Some(draft) = build_location_draft(data, make_entity_id("loc")) else {
            return command_message_response_with_doc(
                "location flow reset.",
                doc().with_block(paragraph_text(
                    "Location flow reset; run create location again.",
                )),
            );
        };

        {
            let mut editor = state.editor_session.lock().await;
            editor.set_location(draft.clone());
        }

        command_response_with_event(
            prepend_notice(data.notice.clone(), location_summary_text(&draft)),
            location_event_from_draft(&draft),
        )
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Numbered, clickable choices from a label slice: `["raw","refined"]` →
/// `1: raw`/`2: refined` with tokens `1`/`2`.
fn numbered_choices(labels: &[&str]) -> Vec<WizardChoice> {
    labels
        .iter()
        .enumerate()
        .map(|(i, label)| WizardChoice::new(format!("{}: {label}", i + 1), (i + 1).to_string()))
        .collect()
}

/// Map a numeric token (`1`-based) to its value in a parallel slice.
fn pick_value<'a>(input: &str, values: &[&'a str]) -> Option<&'a str> {
    let n = input.parse::<usize>().ok()?;
    if (1..=values.len()).contains(&n) {
        Some(values[n - 1])
    } else {
        None
    }
}

const DANGER_LABELS: [&str; 5] = ["safe", "guarded", "risky", "deadly", "let the model decide"];
const DANGER_VALUES: [&str; 5] = ["safe", "guarded", "risky", "deadly", "Unknown"];

fn danger_choices() -> Vec<WizardChoice> {
    numbered_choices(&DANGER_LABELS)
}

/// Resolve a danger token to its canonical level, validated against the schema enum.
fn pick_danger(input: &str) -> Option<String> {
    let value = pick_value(input, &DANGER_VALUES)?;
    LOCATION_DANGER_LEVELS
        .contains(&value)
        .then(|| value.to_string())
}

/// Load the linkable factions (read-only) into the accumulator and jump to the
/// faction-link step, remembering which step asked so `skip` can return there.
async fn enter_faction_link(
    d: &mut WizardData,
    state: &AppState,
    from_step: &'static str,
) -> Result<WizardTransition, String> {
    let factions = load_linkable_factions(state).await?;
    let data = location_data_mut(d);
    data.factions = factions;
    data.faction_link_return = Some(from_step);
    Ok(WizardTransition::Goto("faction_link"))
}

/// Load the existing locations (read-only) into the accumulator and jump to the
/// guildhall anchor step. Mirrors [`enter_faction_link`] for the location picker.
async fn enter_location_anchor(
    d: &mut WizardData,
    state: &AppState,
) -> Result<WizardTransition, String> {
    let locations = load_linkable_locations(state).await?;
    location_data_mut(d).locations = locations;
    Ok(WizardTransition::Goto("guildhall_anchor"))
}

/// Every location the GM can anchor a guildhall to, read-only: unpublished drafts
/// from the DB plus published notes recovered from the vault. Like
/// `entity_link::load_linkable_factions` but each entry carries its subfolder: a
/// draft's comes from its `kind_type` (it will publish there); a published note's
/// from its path.
async fn load_linkable_locations(state: &AppState) -> Result<Vec<LocationAnchorChoice>, String> {
    let database = state.database();
    let rows = state.location_repo().list_all(database.as_ref()).await?;
    let drafts = rows
        .into_iter()
        .map(|row| LocationAnchorChoice {
            sub: location_subfolder(&row.kind_type).unwrap_or("").to_string(),
            name: row.name,
            slug: row.slug,
        })
        .collect();

    let published = tokio::task::spawn_blocking(load_published_locations)
        .await
        .map_err(|err| err.to_string())??;
    Ok(merge_linkable_locations(drafts, published))
}

/// Location-specific merge (drafts win, sorted by name) preserving each entry's
/// subfolder. The faction picker keeps the flat `entity_link::merge_linkable`.
fn merge_linkable_locations(
    mut drafts: Vec<LocationAnchorChoice>,
    published: Vec<LocationAnchorChoice>,
) -> Vec<LocationAnchorChoice> {
    let mut seen: HashSet<String> = drafts
        .iter()
        .map(|choice| choice.slug.to_ascii_lowercase())
        .collect();
    for choice in published {
        if seen.insert(choice.slug.to_ascii_lowercase()) {
            drafts.push(choice);
        }
    }
    drafts.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
    });
    drafts
}

/// Recover published locations from the vault, keeping each note's subfolder (`""`
/// for a flat note, `"settlements"` for `locations/settlements/Foo.md`). Blocking IO.
fn load_published_locations() -> Result<Vec<LocationAnchorChoice>, String> {
    let entries = load_vault_entries_blocking()?;
    Ok(entries_from_refs(&entries, "locations")
        .into_iter()
        .map(|(name, slug, sub)| LocationAnchorChoice { name, slug, sub })
        .collect())
}

/// Run kind-aware generation into the accumulator: the structured branches go
/// through `generate_location_seed_for_wizard`; only the freeform custom lane stays
/// on the one-shot `generate_location_seed`.
async fn generate_location_into(
    d: &mut LocationWizardData,
    state: &AppState,
    hint: Option<&str>,
) -> Result<(), String> {
    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let SeedGeneration { seed, notice } = if d.is_structured() {
        ai.generate_location_seed_for_wizard(
            &d.as_inputs(hint),
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?
    } else {
        ai.generate_location_seed(
            d.custom_prompt(hint),
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?
    };
    d.seed = Some(seed);
    d.notice = notice;
    Ok(())
}

/// Flatten the locked answers into a single bias string so a later
/// `location reroll <field>` reuses the GM's intent (the reroll service merges
/// `seed_prompt`).
fn build_seed_prompt(d: &LocationWizardData) -> Option<String> {
    if d.is_structured() {
        Some(build_wizard_user_prompt(&d.as_inputs(None)))
    } else {
        d.custom_prompt(None)
    }
}

/// Build the editable `LocationDraft` from the accumulator's generated seed +
/// locked answers. `kind_type`/`kind_custom` come from the accumulator (GM-locked
/// at step 1), never the model. Returns `None` if generation never produced a seed.
fn build_location_draft(d: &LocationWizardData, id: String) -> Option<LocationDraftSession> {
    let seed = d.seed.clone()?;
    Some(LocationDraftSession {
        id,
        seed_prompt: build_seed_prompt(d),
        slug: slugify(&seed.name),
        name: seed.name,
        vault_path: String::new(),
        kind_type: d.kind_type.clone(),
        kind_custom: d.kind_custom.clone(),
        visual_description: seed.visual_description,
        history_background: seed.history_background,
        exports: seed.exports,
        tone: seed.tone,
        authority: seed.authority,
        danger_level: seed.danger_level,
        current_tension: seed.current_tension,
        // The guildhall anchor (Q-G3); empty for every other branch. Stays the bare
        // name — Obsidian `[[name]]` resolves by note name regardless of folder.
        location: d.location_anchor.clone().unwrap_or_default(),
        // Wizard-built: request kind-based subfoldering of the published `.md` path.
        wizard_subfoldered: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site_seed() -> LocationSeed {
        LocationSeed {
            name: "Mirecairn".to_string(),
            // The model's pick — the wizard must ignore it and lock from step 1.
            kind_type: "city".to_string(),
            kind_custom: Some("ignored".to_string()),
            visual_description: "Sunken stones.".to_string(),
            history_background: "It fell. It rotted.".to_string(),
            exports: Vec::new(), // suppressed by generation for a site
            tone: "lonely and wet".to_string(),
            authority: "a marsh hag".to_string(),
            danger_level: "deadly".to_string(),
            current_tension: "The water rises.".to_string(),
        }
    }

    #[test]
    fn is_structured_matches_branch_kinds_only() {
        for kind in [
            "hamlet",
            "town",
            "city",
            "ruin",
            "landmark",
            "wilderness",
            "hideout",
            "guildhall",
        ] {
            assert!(is_structured(kind), "{kind} should be structured");
        }
        // Only the freeform custom lane stays one-shot.
        for kind in ["other", "dungeon", ""] {
            assert!(!is_structured(kind), "{kind} should not be structured");
        }
    }

    #[test]
    fn kind_menu_excludes_other_and_dungeon() {
        let menu = kind_menu();
        assert_eq!(menu.len(), 8);
        assert!(!menu.contains(&"other"));
        assert!(!menu.contains(&"dungeon"));
        assert!(menu.contains(&"hideout"));
    }

    #[test]
    fn pick_value_maps_one_based_index() {
        assert_eq!(pick_value("1", &EXPORT_MODE_LABELS), Some("raw"));
        assert_eq!(pick_value("3", &EXPORT_MODE_LABELS), Some("mixed"));
        assert_eq!(pick_value("4", &EXPORT_MODE_LABELS), Some("none"));
        assert_eq!(pick_value("0", &EXPORT_MODE_LABELS), None);
        assert_eq!(pick_value("5", &EXPORT_MODE_LABELS), None);
        assert_eq!(pick_value("x", &EXPORT_MODE_LABELS), None);
    }

    #[test]
    fn pick_danger_resolves_levels_and_let_the_model_decide() {
        assert_eq!(pick_danger("1").as_deref(), Some("safe"));
        assert_eq!(pick_danger("4").as_deref(), Some("deadly"));
        // "let the model decide" → Unknown.
        assert_eq!(pick_danger("5").as_deref(), Some("Unknown"));
        assert_eq!(pick_danger("6"), None);
    }

    #[test]
    fn draft_locks_kind_from_accumulator_and_keeps_suppressed_exports() {
        let mut d = LocationWizardData {
            kind_type: "ruin".to_string(),
            ..Default::default()
        };
        d.seed = Some(site_seed());
        let draft = build_location_draft(&d, "loc_test".to_string()).expect("draft");
        // kind comes from step 1, never the model's pick.
        assert_eq!(draft.kind_type, "ruin");
        assert_eq!(draft.kind_custom, None);
        // exports stay empty (the card omits the row); danger is the seed's locked value.
        assert!(draft.exports.is_empty());
        assert_eq!(draft.danger_level, "deadly");
        assert_eq!(draft.slug, "mirecairn");
        // The wizard path requests kind-based subfoldering of the published `.md`.
        assert!(draft.wizard_subfoldered);
    }

    #[test]
    fn custom_prompt_is_none_for_guildhall_now_structured() {
        // Guildhall is a structured branch, so it never reaches the one-shot
        // `custom_prompt` (only the freeform `other` lane does).
        let d = LocationWizardData {
            kind_type: "guildhall".to_string(),
            ..Default::default()
        };
        assert!(d.custom_prompt(None).is_none());
        assert!(d.is_structured());
    }

    #[test]
    fn custom_prompt_frames_custom_kind_and_seed() {
        let d = LocationWizardData {
            kind_type: "other".to_string(),
            kind_custom: Some("floating market".to_string()),
            custom_seed: Some("tethered to a leviathan".to_string()),
            ..Default::default()
        };
        let prompt = d.custom_prompt(None).expect("custom framing");
        assert!(prompt.contains("floating market"));
        assert!(prompt.contains("leviathan"));
    }

    #[test]
    fn seed_prompt_for_structured_branch_carries_locked_answers() {
        let d = LocationWizardData {
            kind_type: "town".to_string(),
            control: Some("a noble house or lord".to_string()),
            resources: Some("silver ore".to_string()),
            export_mode: Some("refined".to_string()),
            ..Default::default()
        };
        let prompt = build_seed_prompt(&d).expect("seed prompt");
        assert!(prompt.contains("town"));
        assert!(prompt.contains("silver ore"));
    }

    #[test]
    fn control_and_owner_menus_have_no_link_option() {
        // The explicit "link a faction" option was removed; choosing a house / faction
        // archetype now routes to the link step instead. So the menus start at the first
        // archetype (token "1") and expose no token-"0" link entry.
        let data = WizardData::new(LocationWizardData::default());
        for choices in [ControlStep.choices(&data), BaseOwnerStep.choices(&data)] {
            assert_eq!(choices[0].token, "1", "archetypes start at 1");
            assert!(
                !choices.iter().any(|choice| choice.token == "0"),
                "no faction-link option should remain in the menu",
            );
        }
    }

    fn anchor_choice(name: &str, slug: &str, sub: &str) -> LocationAnchorChoice {
        LocationAnchorChoice {
            name: name.to_string(),
            slug: slug.to_string(),
            sub: sub.to_string(),
        }
    }

    #[test]
    fn resolve_anchor_captures_subfolder_for_pick_and_none_for_free_typed() {
        let locations = vec![
            anchor_choice("Silverhall", "silverhall", "settlements"),
            anchor_choice("Greenhollow", "greenhollow", ""),
        ];
        // A published/draft pick carries its subfolder...
        let (name, sub) = resolve_anchor(&locations, "Silverhall").expect("match");
        assert_eq!(name, "Silverhall");
        assert_eq!(sub.as_deref(), Some("settlements"));
        // ...a flat note resolves to no subfolder...
        let (name, sub) = resolve_anchor(&locations, "Greenhollow").expect("match");
        assert_eq!(name, "Greenhollow");
        assert_eq!(sub, None);
        // ...and a free-typed place (no match) is accepted with no subfolder.
        let (name, sub) = resolve_anchor(&locations, "Brand New Place").expect("free text");
        assert_eq!(name, "Brand New Place");
        assert_eq!(sub, None);
    }

    fn faction_link_data(factions: &[(&str, &str)]) -> WizardData {
        WizardData::new(LocationWizardData {
            factions: factions
                .iter()
                .map(|(name, slug)| (name.to_string(), slug.to_string()))
                .collect(),
            ..Default::default()
        })
    }

    #[test]
    fn faction_typeahead_filters_and_ranks_prefix_first() {
        let data = faction_link_data(&[
            ("Crimson Lanterns", "crimson-lanterns"),
            ("The Crimson Court", "the-crimson-court"),
            ("Silver Hand", "silver-hand"),
        ]);
        let tokens: Vec<String> = FactionLinkStep
            .suggest("crim", &data)
            .into_iter()
            .map(|choice| choice.token)
            .collect();
        // Both crimson factions match; the prefix match ranks above the mid-word
        // match, and the unrelated faction is excluded. `skip` is filtered out.
        assert_eq!(
            tokens,
            vec![
                "Crimson Lanterns".to_string(),
                "The Crimson Court".to_string(),
            ]
        );
    }

    #[test]
    fn faction_typeahead_offers_skip_on_empty_query() {
        let data = faction_link_data(&[("Silver Hand", "silver-hand")]);
        let tokens: Vec<String> = FactionLinkStep
            .suggest("", &data)
            .into_iter()
            .map(|choice| choice.token)
            .collect();
        assert!(tokens.contains(&"Silver Hand".to_string()));
        assert!(tokens.contains(&"skip".to_string()));
    }

    fn guildhall_link_data(factions: &[(&str, &str)]) -> WizardData {
        WizardData::new(LocationWizardData {
            faction_link_return: Some("guildhall"),
            factions: factions
                .iter()
                .map(|(name, slug)| (name.to_string(), slug.to_string()))
                .collect(),
            ..Default::default()
        })
    }

    #[test]
    fn faction_link_guildhall_mode_detected_by_return_step() {
        let mut d = LocationWizardData::default();
        assert!(!faction_link_is_guildhall(&d));
        d.faction_link_return = Some("guildhall");
        assert!(faction_link_is_guildhall(&d));
        d.faction_link_return = Some("control");
        assert!(!faction_link_is_guildhall(&d));
    }

    #[test]
    fn guildhall_faction_link_is_mandatory_no_skip() {
        // The link is required for a guildhall, so neither the choices nor the
        // typeahead expose `skip`.
        let data = guildhall_link_data(&[("Silver Hand", "silver-hand")]);
        assert!(FactionLinkStep.choices(&data).is_empty());
        let tokens: Vec<String> = FactionLinkStep
            .suggest("", &data)
            .into_iter()
            .map(|choice| choice.token)
            .collect();
        assert!(tokens.contains(&"Silver Hand".to_string()));
        assert!(!tokens.contains(&"skip".to_string()));
    }

    #[test]
    fn guildhall_role_offers_archetypes_then_skip() {
        let data = WizardData::new(LocationWizardData::default());
        let choices = GuildhallRoleStep.choices(&data);
        assert_eq!(choices[0].token, "1");
        assert!(choices.iter().any(|choice| choice.token == "skip"));
        // Labels and values stay parallel.
        assert_eq!(
            pick_value("1", &GUILDHALL_ROLE_VALUES),
            Some("a counting house or bank")
        );
        assert_eq!(GUILDHALL_ROLE_LABELS.len(), GUILDHALL_ROLE_VALUES.len());
    }

    #[test]
    fn guildhall_anchor_typeahead_lists_locations_without_skip() {
        // The anchor picker is mandatory: it surfaces existing locations and never a
        // `skip`, and exposes no enumerated choices (typeahead-driven).
        let data = WizardData::new(LocationWizardData {
            locations: vec![
                anchor_choice("Silverhall", "silverhall", "settlements"),
                anchor_choice("Greenhollow", "greenhollow", ""),
            ],
            ..Default::default()
        });
        assert!(GuildhallAnchorStep.choices(&data).is_empty());
        let tokens: Vec<String> = GuildhallAnchorStep
            .suggest("", &data)
            .into_iter()
            .map(|choice| choice.token)
            .collect();
        assert!(tokens.contains(&"Silverhall".to_string()));
        assert!(tokens.contains(&"Greenhollow".to_string()));
        assert!(!tokens.contains(&"skip".to_string()));
    }

    #[test]
    fn seed_prompt_for_guildhall_carries_faction_role_and_subfoldered_anchor() {
        // The `@factions/<name>` and `@locations/<sub>/<name>` tokens are what let
        // generation pull each entity's authoritative metadata in for a published note;
        // the subfolder must be present or the path-keyed `@reference` won't resolve.
        let d = LocationWizardData {
            kind_type: "guildhall".to_string(),
            faction_name: Some("Crimson Lanterns".to_string()),
            public_role: Some("a counting house or bank".to_string()),
            location_anchor: Some("Silverhall".to_string()),
            location_anchor_sub: Some("settlements".to_string()),
            ..Default::default()
        };
        let prompt = build_seed_prompt(&d).expect("seed prompt");
        assert!(prompt.contains("guildhall"));
        assert!(prompt.contains("@factions/Crimson Lanterns"));
        assert!(prompt.contains("counting house"));
        assert!(prompt.contains("@locations/settlements/Silverhall"));
    }

    #[test]
    fn seed_prompt_for_guildhall_falls_back_to_name_only_anchor_when_flat() {
        // A flat note / free-typed place has no subfolder -> name-only `@locations/<name>`.
        let d = LocationWizardData {
            kind_type: "guildhall".to_string(),
            faction_name: Some("Crimson Lanterns".to_string()),
            location_anchor: Some("Silverhall".to_string()),
            location_anchor_sub: None,
            ..Default::default()
        };
        let prompt = build_seed_prompt(&d).expect("seed prompt");
        assert!(prompt.contains("@locations/Silverhall"));
        assert!(!prompt.contains("@locations/settlements"));
    }

    #[test]
    fn guildhall_draft_locks_kind_and_suppresses_exports() {
        let mut d = LocationWizardData {
            kind_type: "guildhall".to_string(),
            faction_name: Some("Crimson Lanterns".to_string()),
            ..Default::default()
        };
        let mut seed = site_seed();
        seed.exports = Vec::new(); // generation suppresses exports for a guildhall
        d.seed = Some(seed);
        let draft = build_location_draft(&d, "loc_guild".to_string()).expect("draft");
        assert_eq!(draft.kind_type, "guildhall");
        assert_eq!(draft.kind_custom, None);
        assert!(draft.exports.is_empty());
    }
}
