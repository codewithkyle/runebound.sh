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
use dnd_core::config::load_effective;
use dnd_core::npc::slugify;
use dnd_core::vault::Vault;
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
    build_wizard_user_prompt,
};
use crate::services::vault_ref::{VaultReferenceEntry, load_vault_reference_entries};
use crate::utils::prepend_notice;

use wizard::prompt::wizard_menu;
use wizard::{Wizard, WizardChoice, WizardData, WizardStep, WizardTransition};

// ---------------------------------------------------------------------------
// Accumulator
// ---------------------------------------------------------------------------

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

    // Guildhall (faction-locked public HQ): its public-facing function.
    public_role: Option<String>,

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
                .with_block(paragraph_text(
                    "Type a name for this custom kind (e.g. \"floating market\", \"planar rift\").",
                ));
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

const CONTROL_LABELS: [&str; 5] = [
    "noble house / lord",
    "faction or guild",
    "council / free city",
    "independent / contested",
    "let the model decide",
];
/// Parallel to `CONTROL_LABELS`; the authoritative phrasing fed to generation. An
/// empty string means "let the model decide" → `control = None`.
const CONTROL_VALUES: [&str; 5] = [
    "a noble house or lord",
    "a faction or guild",
    "a ruling council or free city",
    "independent or contested rule",
    "",
];

struct ControlStep;

#[async_trait]
impl WizardStep<AppState> for ControlStep {
    fn id(&self) -> &'static str {
        "control"
    }

    fn summary(&self) -> &'static str {
        "Who controls this settlement? Pick an archetype, or link an existing faction."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Location — Settlement — Control",
            "Who controls it?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        // Linking is always option 0, above the archetypes.
        let mut choices = vec![link_faction_choice(
            "Point control at a faction already in your world",
        )];
        choices.extend(numbered_choices(&CONTROL_LABELS));
        choices
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        if trimmed == "0" || trimmed.eq_ignore_ascii_case("link") {
            return enter_faction_link(d, state, "control").await;
        }
        let Some(value) = pick_value(trimmed, &CONTROL_VALUES) else {
            return Ok(WizardTransition::Stay);
        };
        let data = location_data_mut(d);
        data.control = (!value.is_empty()).then(|| value.to_string());
        // Re-answering control clears any prior faction link.
        data.faction_name = None;
        data.faction_ref = None;
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
            .with_block(paragraph_text(
                "What natural resources are here? (e.g. \"river fish and reed\", \"silver ore\", \"grain and cattle\")",
            ))
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

const EXPORT_MODE_LABELS: [&str; 3] = ["raw", "refined", "mixed"];

struct ExportModeStep;

#[async_trait]
impl WizardStep<AppState> for ExportModeStep {
    fn id(&self) -> &'static str {
        "export_mode"
    }

    fn summary(&self) -> &'static str {
        "Are its exports raw, refined, or mixed? (transport logistics)"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Location — Settlement — Exports",
            "Are its exports raw, refined, or mixed?",
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
        "Whose base is it? Pick an owner, or link an existing faction."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Location — Hideout — Owner",
            "Whose base is it?",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        // Linking is always option 0, above the archetypes.
        let mut choices = vec![link_faction_choice(
            "Point ownership at a faction already in your world",
        )];
        choices.extend(numbered_choices(&BASE_OWNER_LABELS));
        choices
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        if trimmed == "0" || trimmed.eq_ignore_ascii_case("link") {
            return enter_faction_link(d, state, "base_owner").await;
        }
        let Some(value) = pick_value(trimmed, &BASE_OWNER_VALUES) else {
            return Ok(WizardTransition::Stay);
        };
        let data = location_data_mut(d);
        data.base_owner = Some(value.to_string());
        data.faction_name = None;
        data.faction_ref = None;
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
        _state: &AppState,
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
        Ok(WizardTransition::Next)
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

/// How many faction matches the typeahead lists at once (mirrors the `@reference`
/// autocomplete cap), so a huge campaign never floods the suggestion box.
const FACTION_SUGGESTION_LIMIT: usize = 12;

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
        "Type to search your factions by name (autocomplete helps), or skip to pick an archetype."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        let d = location_data(data);
        let mut document = doc().with_block(heading(2, "Create Location — Link a Faction"));
        let count = d.factions.len();
        let noun = if count == 1 { "faction" } else { "factions" };

        if faction_link_is_guildhall(d) {
            // Mandatory: a guildhall must belong to a faction. An unknown name is
            // accepted as a new faction reference, so there is no `skip`.
            let body = if d.factions.is_empty() {
                "A guildhall is the public seat of a faction. No factions exist yet — type the name of the organization that runs this hall.".to_string()
            } else {
                format!(
                    "A guildhall is the public seat of a faction. Start typing to search your {count} {noun} by name — matches autocomplete as you go. Pick one, or type a new name to use a faction that isn't in your world yet."
                )
            };
            return document.with_block(paragraph_text(body));
        }

        if d.factions.is_empty() {
            document = document.with_block(paragraph_with_inlines(vec![
                text_node("No factions exist yet. "),
                command_ref("skip", "skip"),
                text_node(" to pick an archetype instead."),
            ]));
        } else {
            // A long campaign can have hundreds of factions, so search rather than
            // enumerate: typeahead (`suggest`) lists matches as the GM types.
            document = document.with_block(paragraph_with_inlines(vec![
                text_node(format!(
                    "Start typing to search your {count} {noun} by name — matches autocomplete as you go. Pick one, or "
                )),
                command_ref("skip", "skip"),
                text_node(" to choose an archetype instead."),
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
                .with_help("Don't link a faction; pick an archetype instead"),
        ]
    }

    /// Typeahead over the factions loaded on entry: case-insensitive substring
    /// match on the name, prefix matches ranked first, capped. The submitted token
    /// is the display name (readable in the input); `accept` resolves it.
    fn suggest(&self, input: &str, data: &WizardData) -> Vec<WizardChoice> {
        let d = location_data(data);
        let query = input.trim().to_ascii_lowercase();

        let mut matches: Vec<&(String, String)> = d
            .factions
            .iter()
            .filter(|(name, _)| query.is_empty() || name.to_ascii_lowercase().contains(&query))
            .collect();
        matches.sort_by(|(left, _), (right, _)| {
            let left = left.to_ascii_lowercase();
            let right = right.to_ascii_lowercase();
            // Prefix matches outrank mid-word matches; ties break alphabetically.
            right
                .starts_with(&query)
                .cmp(&left.starts_with(&query))
                .then(left.cmp(&right))
        });

        let mut out: Vec<WizardChoice> = matches
            .into_iter()
            .take(FACTION_SUGGESTION_LIMIT)
            .map(|(name, _)| WizardChoice::new(name.clone(), name.clone()))
            .collect();
        // Keep `skip` reachable from typeahead when it isn't filtered out — but never
        // in guildhall mode, where the link is mandatory.
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

        if trimmed.is_empty() {
            // Guildhall: the link is mandatory, so re-prompt instead of skipping.
            return Ok(if guildhall {
                WizardTransition::Stay
            } else {
                WizardTransition::Goto(return_step)
            });
        }
        if !guildhall && trimmed.eq_ignore_ascii_case("skip") {
            return Ok(WizardTransition::Goto(return_step));
        }

        // Exact name/slug wins; otherwise fall back to a unique substring match so a
        // typed fragment resolves without forcing the whole name. A linked faction
        // carries its slug; an unmatched name has no slug.
        let exact = data
            .factions
            .iter()
            .find(|(name, slug)| {
                slug.eq_ignore_ascii_case(trimmed) || name.eq_ignore_ascii_case(trimmed)
            })
            .cloned();
        let (name, faction_ref): (String, Option<String>) = match exact {
            Some((name, slug)) => (name, Some(slug)),
            None => {
                let needle = trimmed.to_ascii_lowercase();
                let mut hits = data
                    .factions
                    .iter()
                    .filter(|(name, _)| name.to_ascii_lowercase().contains(&needle));
                match (hits.next().cloned(), hits.next()) {
                    (Some((name, slug)), None) => (name, Some(slug)),
                    (Some(_), Some(_)) => {
                        return Err(format!(
                            "Several factions match \"{trimmed}\" — pick one from the list or keep typing."
                        ));
                    }
                    // No match: a guildhall accepts the typed text as a new faction
                    // reference (name-only); the optional link rejects it.
                    _ if guildhall => (trimmed.to_string(), None),
                    _ => {
                        return Err(format!(
                            "No faction matches \"{trimmed}\". Type to search, or skip."
                        ));
                    }
                }
            }
        };

        let next = match return_step {
            "base_owner" => {
                data.base_owner = Some(name.clone());
                "base_protection"
            }
            // Guildhall locks `authority` to the faction via `faction_name`; there is
            // no separate control/owner field to set.
            "guildhall" => "guildhall_role",
            _ => {
                data.control = Some(name.clone());
                "resources"
            }
        };
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
                    body: "Geography / trade route? (e.g. \"on a river\", \"landlocked, four days from the coast\")",
                    field: SeedField::Geography,
                }),
                // Site
                Arc::new(SiteFocusStep),
                Arc::new(SiteDangerStep),
                Arc::new(SiteDrawStep),
                Arc::new(GenerateStep {
                    id: "geography_site",
                    title: "Create Location — Site — Map Anchor",
                    body: "Geography / map anchor? (e.g. \"half a day from Greenhollow, deep in the marsh\")",
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
                    body: "Where does the base hide? (e.g. \"the city's sewers\", \"a high mountain pass\")",
                    field: SeedField::Geography,
                }),
                // Guildhall (faction link runs first, from KindStep)
                Arc::new(GuildhallRoleStep),
                Arc::new(GenerateStep {
                    id: "geography_guildhall",
                    title: "Create Location — Guildhall — Map Anchor",
                    body: "Where does the hall stand? (e.g. \"on the merchant quarter's main square\", \"the old temple district\")",
                    field: SeedField::Geography,
                }),
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

/// The leading "0: link an existing faction" entry shared by the control/owner
/// menus, so linking is always option 0 (above the numbered archetypes).
fn link_faction_choice(help: &'static str) -> WizardChoice {
    WizardChoice::new("0: link an existing faction", "0").with_help(help)
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

/// Every faction the GM can link, read-only: unpublished drafts from the DB plus
/// published notes recovered from the vault (those were reaped from the DB at
/// publish time and now live only as `.md` files). Deduped by slug, sorted by name.
async fn load_linkable_factions(state: &AppState) -> Result<Vec<(String, String)>, String> {
    let database = state.database();
    let rows = state.faction_repo().list_all(database.as_ref()).await?;
    let mut factions: Vec<(String, String)> =
        rows.into_iter().map(|row| (row.name, row.slug)).collect();

    // Reading the vault is recursive, blocking IO; keep it off the async runtime.
    let published = tokio::task::spawn_blocking(load_published_faction_names)
        .await
        .map_err(|err| err.to_string())??;

    let mut seen: HashSet<String> = factions
        .iter()
        .map(|(_, slug)| slug.to_ascii_lowercase())
        .collect();
    for (name, slug) in published {
        if seen.insert(slug.to_ascii_lowercase()) {
            factions.push((name, slug));
        }
    }
    factions.sort_by(|left, right| {
        left.0
            .to_ascii_lowercase()
            .cmp(&right.0.to_ascii_lowercase())
    });
    Ok(factions)
}

/// Recover published factions from the Obsidian vault's `factions/` folder. Blocking
/// IO (recursive `read_dir`) — only call inside `spawn_blocking`.
fn load_published_faction_names() -> Result<Vec<(String, String)>, String> {
    let loaded = load_effective().map_err(|err| err.to_string())?;
    let Some(vault_path) = loaded.effective.vault.path else {
        return Ok(Vec::new());
    };
    let vault = Vault::new(vault_path);
    if vault.ensure_root_exists().is_err() {
        return Ok(Vec::new());
    }
    let entries = load_vault_reference_entries(&vault)?;
    Ok(faction_entries_from_refs(&entries))
}

/// Extract `(display name, slug)` for each faction note under the vault's
/// `factions/` folder. The display name is the file stem; the slug is derived from
/// it, since published notes carry no DB row. Pure, so it's unit-testable.
fn faction_entries_from_refs(entries: &[VaultReferenceEntry]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        let Some(path) = entry.markdown_path.as_deref() else {
            continue;
        };
        let Some((dir, file)) = path.split_once('/') else {
            continue;
        };
        if !dir.eq_ignore_ascii_case("factions") {
            continue;
        }
        // Publish writes `factions/<Name>.md`; ignore anything nested deeper.
        if file.contains('/') {
            continue;
        }
        let name = std::path::Path::new(file)
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(name) = name {
            out.push((name.to_string(), slugify(name)));
        }
    }
    out
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
        assert_eq!(pick_value("0", &EXPORT_MODE_LABELS), None);
        assert_eq!(pick_value("4", &EXPORT_MODE_LABELS), None);
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
    fn link_faction_is_always_option_zero() {
        let data = WizardData::new(LocationWizardData::default());
        for choices in [ControlStep.choices(&data), BaseOwnerStep.choices(&data)] {
            let first = &choices[0];
            assert_eq!(first.token, "0", "link must submit token 0");
            assert!(
                first
                    .label
                    .to_lowercase()
                    .contains("link an existing faction"),
                "option 0 should be the faction link, got {:?}",
                first.label
            );
            // The archetypes follow, still numbered from 1.
            assert_eq!(choices[1].token, "1");
        }
    }

    fn ref_file(key: &str) -> VaultReferenceEntry {
        VaultReferenceEntry {
            key: key.to_string(),
            key_lower: key.to_ascii_lowercase(),
            markdown_path: Some(format!("{key}.md")),
            is_dir: false,
        }
    }

    #[test]
    fn faction_entries_keep_top_level_faction_notes_only() {
        let entries = vec![
            ref_file("factions/Crimson Lanterns"),
            ref_file("factions/sub/Nested Note"), // nested → ignored
            ref_file("npcs/Lirael Drake"),        // wrong folder → ignored
            VaultReferenceEntry {
                key: "factions/".to_string(),
                key_lower: "factions/".to_string(),
                markdown_path: None,
                is_dir: true, // directory → ignored
            },
        ];
        let out = faction_entries_from_refs(&entries);
        assert_eq!(
            out,
            vec![(
                "Crimson Lanterns".to_string(),
                "crimson-lanterns".to_string()
            )]
        );
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
    fn seed_prompt_for_guildhall_carries_faction_reference_and_role() {
        // The `@factions/<name>` token is what lets generation pull the faction's
        // authoritative metadata in for a published faction.
        let d = LocationWizardData {
            kind_type: "guildhall".to_string(),
            faction_name: Some("Crimson Lanterns".to_string()),
            public_role: Some("a counting house or bank".to_string()),
            ..Default::default()
        };
        let prompt = build_seed_prompt(&d).expect("seed prompt");
        assert!(prompt.contains("guildhall"));
        assert!(prompt.contains("@factions/Crimson Lanterns"));
        assert!(prompt.contains("counting house"));
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
