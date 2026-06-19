//! The location wizard: the guided `create location` flow expressed as declarative
//! `WizardStep`s on the shared engine (docs/architecture.md §4). Step 1 picks the
//! GM-locked `kind_type`, which routes to one of four branches — Settlement, Site,
//! Hideout, or the minimal/custom lane — each of which ends by generating the
//! LLM-derived fields *under* the GM's locked answers and converging on the same
//! `LocationDraft` the one-shot `create_location` produced. So `save`/`reroll`/the
//! card UI all keep working unchanged (the dungeon model applied to location).
//!
//! The locked answers live only in this accumulator; they are baked into the
//! generated prose and the derived/locked fields, and flattened into `seed_prompt`
//! as reroll bias. Nothing new is persisted (no draft/row/migration changes).

use std::sync::Arc;

use async_trait::async_trait;
use dnd_core::npc::slugify;
use runebound_models::drafts::{CardFooter, location_entity_card};
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
use crate::utils::prepend_notice;

use wizard::prompt::{action_row, wizard_menu};
use wizard::{Wizard, WizardChoice, WizardData, WizardStep, WizardTransition};

// ---------------------------------------------------------------------------
// Accumulator
// ---------------------------------------------------------------------------

/// The per-flow answers; the cursor/history live in the engine's `WizardSession`.
/// `seed`/`notice` carry the generated result and a one-shot capacity notice to the
/// review screen.
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

    // GM-locked danger for Site + Hideout (Q-S2 / Q-H3)
    danger_lock: Option<String>,

    // Shared optional map anchor (Q-D / Q-S4 / Q-H5)
    geography: Option<String>,

    // Minimal / custom seed (free text)
    custom_seed: Option<String>,

    // Read-only faction link (Q-A / Q-H1): the loaded options, plus the chosen
    // faction's canonical name + slug, and which step requested the link.
    factions: Vec<(String, String)>,
    faction_name: Option<String>,
    faction_ref: Option<String>,
    faction_link_return: Option<&'static str>,

    // Generated seed held for the review screen, plus a one-shot notice.
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
    /// The structured branches (Settlement/Site/Hideout) generate under locked
    /// answers; `guildhall`/`other` stay on the one-shot lane.
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
            danger_lock: self.danger_lock.clone(),
            geography: self.geography.clone(),
            faction_name: self.faction_name.clone(),
            hint: hint.map(str::to_string),
        }
    }

    /// The one-shot prompt for the minimal/custom branch: the kind framing plus the
    /// GM's free-text seed and any reroll hint.
    fn custom_prompt(&self, hint: Option<&str>) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        match self.kind_type.as_str() {
            "guildhall" => {
                parts.push("A guildhall — a public, owned guild headquarters.".to_string())
            }
            "other" => {
                if let Some(custom) = trimmed_opt(&self.kind_custom) {
                    parts.push(format!("A {custom}."));
                }
            }
            _ => {}
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
        "hamlet" | "town" | "city" | "ruin" | "landmark" | "wilderness" | "hideout"
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
            "Create Location — Step 1 — Kind",
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
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        let data = location_data_mut(d);

        // Second phase of option 0: capture the custom kind name, then generate.
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

        let menu = kind_menu();
        let Ok(n) = trimmed.parse::<usize>() else {
            return Ok(WizardTransition::Stay);
        };
        if !(1..=menu.len()).contains(&n) {
            return Ok(WizardTransition::Stay);
        }
        let kind = menu[n - 1];
        data.kind_type = kind.to_string();
        let target = match kind {
            "hamlet" | "town" | "city" => "control",
            "ruin" | "landmark" | "wilderness" => "site_focus",
            "hideout" => "base_owner",
            "guildhall" => "custom_seed", // minimal branch: owned-but-public
            _ => return Ok(WizardTransition::Stay),
        };
        Ok(WizardTransition::Goto(target))
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
        let mut choices = numbered_choices(&CONTROL_LABELS);
        choices.push(
            WizardChoice::new("link an existing faction", "link")
                .with_help("Point control at a faction already in your world"),
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
        if trimmed.eq_ignore_ascii_case("link") {
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
        let mut choices = numbered_choices(&BASE_OWNER_LABELS);
        choices.push(
            WizardChoice::new("link an existing faction", "link")
                .with_help("Point ownership at a faction already in your world"),
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
        if trimmed.eq_ignore_ascii_case("link") {
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
// Generate steps — each branch's final (optional) step + the custom seed
// ---------------------------------------------------------------------------

/// Which accumulator field a [`GenerateStep`] records before generating.
#[derive(Clone, Copy)]
enum SeedField {
    Geography,
    CustomSeed,
}

/// The terminal step of every branch: record the (optional, skippable) free text,
/// run generation under the locked answers, then jump to the shared review.
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
        Ok(WizardTransition::Goto("review"))
    }
}

// ---------------------------------------------------------------------------
// Review — shared terminal
// ---------------------------------------------------------------------------

struct ReviewStep;

#[async_trait]
impl WizardStep<AppState> for ReviewStep {
    fn id(&self) -> &'static str {
        "review"
    }

    fn awaiting_llm_label(&self) -> Option<&'static str> {
        Some("generating location")
    }

    fn summary(&self) -> &'static str {
        "Review the location. Continue to open it in the editor, or reroll for a new one."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        let d = location_data(data);
        let Some(draft) = build_location_draft(d, String::new()) else {
            return doc().with_block(paragraph_text("No location generated yet."));
        };
        let mut document = doc();
        if let Some(notice) = &d.notice {
            document = document.with_block(paragraph_text(notice.clone()));
        }
        document = document.with_block(heading(2, "Create Location — Review"));
        for block in location_entity_card(&draft, CardFooter::Hide).blocks {
            document = document.with_block(block);
        }
        document.with_block(action_row(&self.choices(data)))
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![
            WizardChoice::new("continue", "continue").with_help("Open this location in the editor"),
            WizardChoice::new("reroll", "reroll")
                .with_help("Regenerate the location (optionally `reroll <hint>`)"),
            WizardChoice::new("cancel", "cancel").with_help("Discard this location and exit"),
        ]
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        state: &AppState,
    ) -> Result<WizardTransition, String> {
        let mut parts = input.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
        let rest = parts.next().unwrap_or("").trim();

        match cmd.as_str() {
            "continue" | "accept" => Ok(WizardTransition::Complete),
            "reroll" | "redo" => {
                let hint = (!rest.is_empty()).then_some(rest);
                generate_location_into(location_data_mut(d), state, hint).await?;
                Ok(WizardTransition::Stay)
            }
            _ => Ok(WizardTransition::Stay),
        }
    }
}

// ---------------------------------------------------------------------------
// Faction link — shared, read-only (Q-A / Q-H1)
// ---------------------------------------------------------------------------

struct FactionLinkStep;

#[async_trait]
impl WizardStep<AppState> for FactionLinkStep {
    fn id(&self) -> &'static str {
        "faction_link"
    }

    fn summary(&self) -> &'static str {
        "Pick an existing faction to own this place (read-only), or skip to choose an archetype."
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        let d = location_data(data);
        let mut document = doc().with_block(heading(2, "Create Location — Link a Faction"));
        if d.factions.is_empty() {
            document = document.with_block(paragraph_with_inlines(vec![
                text_node("No factions exist yet. "),
                command_ref("skip", "skip"),
                text_node(" to pick an archetype instead."),
            ]));
        } else {
            document = document
                .with_block(paragraph_text("Pick a faction to own this place:"))
                .with_block(wizard::prompt::choice_lines(&self.choices(data)));
        }
        document
    }

    fn choices(&self, data: &WizardData) -> Vec<WizardChoice> {
        let d = location_data(data);
        let mut choices: Vec<WizardChoice> = d
            .factions
            .iter()
            .map(|(name, slug)| WizardChoice::new(name.clone(), slug.clone()))
            .collect();
        choices.push(
            WizardChoice::new("skip", "skip")
                .with_help("Don't link a faction; pick an archetype instead"),
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
        let data = location_data_mut(d);
        let return_step = data.faction_link_return.unwrap_or("control");

        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("skip") {
            return Ok(WizardTransition::Goto(return_step));
        }

        let chosen = data
            .factions
            .iter()
            .find(|(name, slug)| {
                slug.eq_ignore_ascii_case(trimmed) || name.eq_ignore_ascii_case(trimmed)
            })
            .cloned();
        let Some((name, slug)) = chosen else {
            return Ok(WizardTransition::Stay);
        };

        let next = if return_step == "base_owner" {
            data.base_owner = Some(name.clone());
            "base_protection"
        } else {
            data.control = Some(name.clone());
            "resources"
        };
        data.faction_name = Some(name);
        data.faction_ref = Some(slug);
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
                // Minimal / custom
                Arc::new(GenerateStep {
                    id: "custom_seed",
                    title: "Create Location — Describe It",
                    body: "Describe this place in a sentence or two.",
                    field: SeedField::CustomSeed,
                }),
                // Shared terminal + faction link
                Arc::new(ReviewStep),
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

    /// `continue` at review: build the `LocationDraft` from the generated seed +
    /// locked answers and open it in the location editor — the same hand-off the
    /// one-shot `create_location` performs. No LLM call here.
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

/// Load the existing factions (read-only) into the accumulator and jump to the
/// faction-link step, remembering which step asked so `skip` can return there.
async fn enter_faction_link(
    d: &mut WizardData,
    state: &AppState,
    from_step: &'static str,
) -> Result<WizardTransition, String> {
    let database = state.database();
    let rows = state.faction_repo().list_all(database.as_ref()).await?;
    let data = location_data_mut(d);
    data.factions = rows.into_iter().map(|row| (row.name, row.slug)).collect();
    data.faction_link_return = Some(from_step);
    Ok(WizardTransition::Goto("faction_link"))
}

/// Run kind-aware generation into the accumulator: the structured branches go
/// through `generate_location_seed_for_wizard`; guildhall/custom stay one-shot.
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
        ] {
            assert!(is_structured(kind), "{kind} should be structured");
        }
        for kind in ["guildhall", "other", "dungeon", ""] {
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
    fn custom_prompt_frames_guildhall_even_when_skipped() {
        let d = LocationWizardData {
            kind_type: "guildhall".to_string(),
            ..Default::default()
        };
        let prompt = d.custom_prompt(None).expect("guildhall framing");
        assert!(prompt.to_lowercase().contains("guildhall"));
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
}
