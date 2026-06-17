//! The dungeon wizard: the guided `create dungeon` flow expressed as declarative
//! `WizardStep`s on the engine (see docs/create-wizard-refactor.md §5 for the
//! step map). The services (`AiGenerationService`, `roll_dungeon_content_plan`)
//! and the `finalize` hand-off into the dungeon editor are reused verbatim.
//!
//! The step headings are plain display text: the frontend spinner keys off the
//! structured `WizardView` signal (step id + `awaiting_llm_label`), not the
//! heading strings, so they carry no marker coupling.

use std::sync::Arc;

use async_trait::async_trait;
use dnd_core::npc::slugify;
use runebound_models::apply_plan_meta_to_beats;
use runebound_models::dungeon_plan::{roll_dungeon_content_plan, DungeonContentPlan};
use runebound_models::output::{
    code, command_ref, doc, heading, image, list, paragraph_text, paragraph_with_inlines,
    text_node, InlineNode, OutputDoc,
};
use runebound_models::utils::{make_entity_id, DUNGEON_FUNCTIONS, DUNGEON_TOPOLOGIES};

use crate::app_state::{AppState, DungeonDraftSession};
use crate::commands::{dungeon_event_from_draft, dungeon_summary_text};
use crate::entities::common::{
    command_message_response_with_doc, command_response_with_event, CommandResult,
};
use crate::entities::EntityKind;
use crate::services::ai_generation::{AiGenerationService, DungeonStory, SeedGeneration};
use crate::utils::{normalize_unknown_text, prepend_notice};

use super::prompt::{action_row, wizard_menu};
use super::session::WizardData;
use super::wizard::{Wizard, WizardChoice, WizardStep, WizardTransition};

/// Anchor (room) content types the GM may pin with `set room`. Excludes the
/// overlay/tint types, which are never a room on their own.
const SETTABLE_ROOM_TYPES: [&str; 8] = [
    "combat",
    "puzzle",
    "cache",
    "offshoot",
    "sidekick",
    "oddity",
    "forge",
    "ability_check",
];

// ---------------------------------------------------------------------------
// Accumulator
// ---------------------------------------------------------------------------

/// The dungeon wizard's accumulator — the per-flow answers, with the cursor and
/// history owned by the engine's `WizardSession`. `notice`/`story_notice` are
/// transient (shown once on the next render, not persisted into the draft).
#[derive(Debug, Clone, Default)]
struct DungeonWizardData {
    premise: Option<String>,
    tone: Option<String>,
    twist: Option<String>,
    context: String,
    topology: Option<String>,
    plan: Option<DungeonContentPlan>,
    story_name: Option<String>,
    story_location: Option<String>,
    story_text: Option<String>,
    /// Transient usage hint after a bad `set room` (cleared on the next action).
    notice: Option<String>,
    /// Carries the LLM generation notice from story generation to the review prompt.
    story_notice: Option<String>,
}

fn dungeon_data(d: &WizardData) -> &DungeonWizardData {
    d.downcast_ref::<DungeonWizardData>()
        .expect("dungeon wizard data")
}

fn dungeon_data_mut(d: &mut WizardData) -> &mut DungeonWizardData {
    d.downcast_mut::<DungeonWizardData>()
        .expect("dungeon wizard data")
}

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

struct PremiseStep;

#[async_trait]
impl WizardStep for PremiseStep {
    fn id(&self) -> &'static str {
        "premise"
    }

    fn prompt(&self, _data: &WizardData) -> OutputDoc {
        doc()
            .with_block(heading(2, "Create Dungeon — Step 1 of 6 — Premise"))
            .with_block(paragraph_with_inlines(vec![
                text_node("Enter a one-line premise, or "),
                command_ref("generate", "generate"),
                text_node(" to have the oracle invent one."),
            ]))
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![WizardChoice::new("generate", "generate")]
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        if input.is_empty() {
            return Ok(WizardTransition::Stay);
        }
        let d = dungeon_data_mut(d);
        d.premise = if input.eq_ignore_ascii_case("generate") {
            None
        } else {
            Some(input.to_string())
        };
        Ok(WizardTransition::Next)
    }
}

struct ToneStep;

#[async_trait]
impl WizardStep for ToneStep {
    fn id(&self) -> &'static str {
        "tone"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Dungeon — Step 2 of 6 — Tone",
            "Choose the overall emotional polarity:",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![
            WizardChoice::new("1: Tragedy", "1"),
            WizardChoice::new("2: Comedy", "2"),
        ]
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let tone = match input {
            "1" => "tragedy",
            "2" => "comedy",
            _ => return Ok(WizardTransition::Stay),
        };
        dungeon_data_mut(d).tone = Some(tone.to_string());
        Ok(WizardTransition::Next)
    }
}

struct TwistStep;

#[async_trait]
impl WizardStep for TwistStep {
    fn id(&self) -> &'static str {
        "twist"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        wizard_menu(
            "Create Dungeon — Step 3 of 6 — Twist",
            "Choose the shape of the middle beats:",
            &self.choices(data),
        )
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![
            WizardChoice::new("1: False victory", "1"),
            WizardChoice::new("2: False defeat", "2"),
            WizardChoice::new("3: Neither", "3"),
        ]
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let twist = match input {
            "1" => "false_victory",
            "2" => "false_defeat",
            "3" => "neither",
            _ => return Ok(WizardTransition::Stay),
        };
        dungeon_data_mut(d).twist = Some(twist.to_string());
        Ok(WizardTransition::Next)
    }
}

struct ContextStep;

#[async_trait]
impl WizardStep for ContextStep {
    fn id(&self) -> &'static str {
        "context"
    }

    fn prompt(&self, _data: &WizardData) -> OutputDoc {
        doc()
            .with_block(heading(2, "Create Dungeon — Step 4 of 6 — Context"))
            .with_block(paragraph_with_inlines(vec![
                text_node("Add references/constraints to seed the oracle (or "),
                command_ref("skip", "skip"),
                text_node("). You may include @references to vault documents."),
            ]))
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![WizardChoice::new("skip", "skip")]
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        dungeon_data_mut(d).context = if input.is_empty() || input.eq_ignore_ascii_case("skip") {
            String::new()
        } else {
            input.to_string()
        };
        Ok(WizardTransition::Next)
    }
}

struct TopologyStep;

#[async_trait]
impl WizardStep for TopologyStep {
    fn id(&self) -> &'static str {
        "topology"
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        // Heading, the topology illustration, then the prompt + clickable options.
        doc()
            .with_block(heading(2, "Create Dungeon — Step 5 of 6 — Topology"))
            .with_block(image(
                "topology",
                "The nine dungeon topologies — each named form with its entrance (E) marked",
            ))
            .with_block(paragraph_text("Pick one of the nine forms, or 0 for none:"))
            .with_block(super::prompt::choice_lines(&self.choices(data)))
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        // DUNGEON_TOPOLOGIES[0] is "none"; render it as option 0, the rest 1..=9.
        let mut choices = vec![WizardChoice::new("0: None (lay it out freely)", "0")];
        for (i, name) in DUNGEON_TOPOLOGIES.iter().enumerate().skip(1) {
            choices.push(WizardChoice::new(format!("{i}: {name}"), i.to_string()));
        }
        choices
    }

    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _state: &AppState,
    ) -> Result<WizardTransition, String> {
        let index: usize = match input.parse() {
            Ok(value) if value < DUNGEON_TOPOLOGIES.len() => value,
            _ => return Ok(WizardTransition::Stay),
        };
        let d = dungeon_data_mut(d);
        d.topology = Some(DUNGEON_TOPOLOGIES[index].to_string());
        // The content-type roll is local and instant; surface it as its own review
        // screen before spending an LLM call on the story.
        roll_plan_into(d);
        Ok(WizardTransition::Goto("plan_review"))
    }
}

struct PlanReviewStep;

#[async_trait]
impl WizardStep for PlanReviewStep {
    fn id(&self) -> &'static str {
        "plan_review"
    }

    fn awaiting_llm_label(&self) -> Option<&'static str> {
        Some("generating story")
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        let d = dungeon_data(data);
        let Some(plan) = &d.plan else {
            return doc().with_block(paragraph_text("No content plan rolled yet."));
        };
        let items: Vec<Vec<InlineNode>> = plan_room_lines(plan)
            .into_iter()
            .map(|line| vec![text_node(line)])
            .collect();
        let mut document = doc().with_block(heading(2, "Create Dungeon — Step 6 of 6 — Room Plan"));
        if let Some(notice) = &d.notice {
            document = document.with_block(paragraph_text(notice.clone()));
        }
        document
            .with_block(paragraph_text("The dice rolled these rooms for your dungeon:"))
            .with_block(list(items))
            .with_block(paragraph_with_inlines(vec![
                text_node("Pin a room with "),
                code("set room <room> <type>"),
                text_node(" (by number or name)."),
            ]))
            .with_block(action_row(&self.choices(data)))
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![
            WizardChoice::new("continue", "continue"),
            WizardChoice::new("reroll", "reroll"),
            WizardChoice::new("cancel", "cancel"),
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

        // Clear any stale usage hint from a previous turn.
        dungeon_data_mut(d).notice = None;

        match cmd.as_str() {
            "continue" | "accept" => {
                generate_story(dungeon_data_mut(d), None, state).await?;
                Ok(WizardTransition::Next)
            }
            "reroll" | "redo" => {
                roll_plan_into(dungeon_data_mut(d));
                Ok(WizardTransition::Stay)
            }
            "set" => {
                if let Err(usage) = set_room_type(dungeon_data_mut(d), rest) {
                    dungeon_data_mut(d).notice = Some(usage);
                }
                Ok(WizardTransition::Stay)
            }
            _ => Ok(WizardTransition::Stay),
        }
    }
}

struct StoryReviewStep;

#[async_trait]
impl WizardStep for StoryReviewStep {
    fn id(&self) -> &'static str {
        "story_review"
    }

    fn awaiting_llm_label(&self) -> Option<&'static str> {
        Some("generating dungeon")
    }

    fn prompt(&self, data: &WizardData) -> OutputDoc {
        let d = dungeon_data(data);
        let (Some(name), Some(story)) = (&d.story_name, &d.story_text) else {
            return doc().with_block(paragraph_text("No story generated yet."));
        };
        let location = d.story_location.clone().unwrap_or_default();
        let mut document = doc();
        if let Some(notice) = &d.story_notice {
            document = document.with_block(paragraph_text(notice.clone()));
        }
        document
            .with_block(heading(2, "Create Dungeon — Story"))
            .with_block(heading(3, format!("{name} — {location}")))
            .with_block(paragraph_text(story.clone()))
            .with_block(action_row(&self.choices(data)))
            .with_block(paragraph_with_inlines(vec![
                text_node("Tip: "),
                code("reroll <hint>"),
                text_node(" to steer a new story."),
            ]))
    }

    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        vec![
            WizardChoice::new("continue", "continue"),
            WizardChoice::new("reroll", "reroll"),
            WizardChoice::new("cancel", "cancel"),
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
                // Keep the locked plan; only the prose is regenerated (optionally steered).
                let hint = (!rest.is_empty()).then_some(rest);
                generate_story(dungeon_data_mut(d), hint, state).await?;
                Ok(WizardTransition::Stay)
            }
            _ => Ok(WizardTransition::Stay),
        }
    }
}

// ---------------------------------------------------------------------------
// Wizard
// ---------------------------------------------------------------------------

pub struct DungeonWizard {
    steps: Vec<Arc<dyn WizardStep>>,
}

impl DungeonWizard {
    pub fn new() -> Self {
        Self {
            steps: vec![
                Arc::new(PremiseStep),
                Arc::new(ToneStep),
                Arc::new(TwistStep),
                Arc::new(ContextStep),
                Arc::new(TopologyStep),
                Arc::new(PlanReviewStep),
                Arc::new(StoryReviewStep),
            ],
        }
    }
}

#[async_trait]
impl Wizard for DungeonWizard {
    fn id(&self) -> &'static str {
        "dungeon"
    }

    fn title(&self) -> &'static str {
        "Create Dungeon"
    }

    fn steps(&self) -> &[Arc<dyn WizardStep>] {
        &self.steps
    }

    fn seed(&self) -> WizardData {
        WizardData::new(DungeonWizardData::default())
    }

    /// `continue` at the story review: structure the locked story (Pass 2), assemble
    /// the seed, and open the editable dungeon draft — the same hand-off the former
    /// `finalize_dungeon` did. The engine resets the session afterward.
    async fn finalize(&self, state: &AppState, d: &WizardData) -> CommandResult {
        let d = dungeon_data(d);
        let story = match (d.story_name.clone(), d.story_text.clone()) {
            (Some(name), Some(text)) => Some(DungeonStory {
                name,
                location: d.story_location.clone().unwrap_or_default(),
                story: text,
            }),
            _ => None,
        };
        let tone = d.tone.clone().unwrap_or_else(|| "tragedy".to_string());
        let twist = d.twist.clone().unwrap_or_else(|| "neither".to_string());
        let topology = d.topology.clone().unwrap_or_else(|| "none".to_string());

        let (Some(plan), Some(story)) = (d.plan.clone(), story) else {
            return command_message_response_with_doc(
                "dungeon flow reset.",
                doc().with_block(paragraph_text(
                    "Dungeon flow reset; run create dungeon again.",
                )),
            );
        };

        let ai = AiGenerationService;
        let database = state.database();
        let generation_repo = state.generation_repo();
        let SeedGeneration { seed, notice } = ai
            .structure_dungeon_story(
                &plan,
                &story,
                &tone,
                &twist,
                &topology,
                &state.workspace_root,
                database.as_ref(),
                generation_repo.as_ref(),
            )
            .await?;

        // seed_prompt persists the premise+context bias so later rerolls reuse it.
        let seed_prompt = build_seed_prompt(d.premise.as_deref(), &d.context);

        // Stamp the rolled overlay + faction tint onto the beats so they persist and
        // a later whole-dungeon reroll can honor them.
        let mut beats = seed.into_beats();
        apply_plan_meta_to_beats(&mut beats, &plan);

        let draft = DungeonDraftSession {
            id: make_entity_id("dungeon"),
            seed_prompt,
            name: seed.name.trim().to_string(),
            slug: slugify(seed.name.trim()),
            vault_path: String::new(),
            location: normalize_unknown_text(&seed.location),
            story: story.story.clone(),
            premise: normalize_unknown_text(&seed.premise),
            topology,
            tone,
            twist,
            beats,
        };

        {
            let mut editor = state.editor_session.lock().await;
            editor.set_dungeon(draft.clone());
            editor.clear_kind(EntityKind::Npc);
            editor.clear_kind(EntityKind::Location);
            editor.clear_kind(EntityKind::Faction);
            editor.clear_kind(EntityKind::Item);
            editor.clear_kind(EntityKind::Event);
            editor.clear_kind(EntityKind::God);
        }

        command_response_with_event(
            prepend_notice(notice, dungeon_summary_text(&draft)),
            dungeon_event_from_draft(&draft),
        )
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Roll a fresh content plan into the accumulator. A new plan invalidates any story
/// reviewed against the previous roll.
fn roll_plan_into(d: &mut DungeonWizardData) {
    d.plan = Some(roll_dungeon_content_plan(plan_seed()));
    d.story_name = None;
    d.story_location = None;
    d.story_text = None;
    d.story_notice = None;
}

/// Run Pass 1 against the locked plan and store the story for review. Shared by
/// `continue` at the plan screen and `reroll [hint]` at the story screen — the plan
/// stays fixed, so story variety comes from the prose, not a re-roll.
async fn generate_story(
    d: &mut DungeonWizardData,
    extra_prompt: Option<&str>,
    state: &AppState,
) -> Result<(), String> {
    let Some(plan) = d.plan.clone() else {
        return Err("no content plan to write a story for".to_string());
    };
    let premise = d.premise.clone();
    let context = d.context.clone();
    let tone = d.tone.clone().unwrap_or_else(|| "tragedy".to_string());
    let twist = d.twist.clone().unwrap_or_else(|| "neither".to_string());
    let topology = d.topology.clone().unwrap_or_else(|| "none".to_string());

    let ai = AiGenerationService;
    let database = state.database();
    let generation_repo = state.generation_repo();
    let SeedGeneration {
        seed: story,
        notice,
    } = ai
        .generate_dungeon_story(
            &plan,
            premise,
            &context,
            &tone,
            &twist,
            &topology,
            extra_prompt,
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;

    d.story_name = Some(story.name.clone());
    d.story_location = Some(story.location.clone());
    d.story_text = Some(story.story.clone());
    d.story_notice = notice;
    Ok(())
}

/// `set room <room> <type>`: pin one beat's room type. Returns `Err(usage)` on bad
/// input. The room may be a number (1-5) or a function name (Entrance, Puzzle, …).
fn set_room_type(d: &mut DungeonWizardData, rest: &str) -> Result<(), String> {
    let mut tokens = rest.split_whitespace();
    // Require the explicit `room` keyword: `set room <room> <type>`.
    if !tokens.next().is_some_and(|t| t.eq_ignore_ascii_case("room")) {
        return Err(set_room_usage());
    }
    let room = tokens.next().unwrap_or("");
    // Allow a spelled-out type ("ability check") by joining the remaining tokens.
    let raw_type = tokens.collect::<Vec<_>>().join("_");

    let Some(index) = resolve_room_index(room) else {
        return Err(set_room_usage());
    };
    let normalized = raw_type.trim().to_ascii_lowercase().replace('-', "_");
    if !SETTABLE_ROOM_TYPES.contains(&normalized.as_str()) {
        return Err(set_room_usage());
    }
    let Some(plan) = d.plan.as_mut() else {
        return Err("no content plan to edit".to_string());
    };
    plan.anchors[index] = normalized;
    Ok(())
}

/// Resolve a room argument to a beat index: a 1-5 number, or a function name
/// (case-insensitive) like "entrance" or "climax".
fn resolve_room_index(room: &str) -> Option<usize> {
    if let Ok(n) = room.parse::<usize>() {
        return if (1..=5).contains(&n) { Some(n - 1) } else { None };
    }
    DUNGEON_FUNCTIONS
        .iter()
        .position(|name| name.eq_ignore_ascii_case(room))
}

fn set_room_usage() -> String {
    let rooms = DUNGEON_FUNCTIONS
        .iter()
        .enumerate()
        .map(|(i, name)| format!("{} {name}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let types = SETTABLE_ROOM_TYPES.join(", ");
    format!(
        "Usage: set room <room> <type>. Rooms: {rooms}. Types: {types}. (Or continue, reroll, cancel.)"
    )
}

/// Title-case a snake_case content type: "ability_check" -> "Ability Check".
fn content_label(content_type: &str) -> String {
    content_type
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One "Function — Type" line per beat, plus any overlay/faction tint.
fn plan_room_lines(plan: &DungeonContentPlan) -> Vec<String> {
    let mut lines: Vec<String> = plan
        .anchors
        .iter()
        .enumerate()
        .map(|(i, anchor)| {
            format!("{}. {} — {}", i + 1, DUNGEON_FUNCTIONS[i], content_label(anchor))
        })
        .collect();
    if let Some(overlay) = &plan.overlay {
        lines.push(format!(
            "Overlay: {} (layered on the {})",
            content_label(&overlay.overlay_type),
            DUNGEON_FUNCTIONS[overlay.beat_index]
        ));
    }
    if plan.factions {
        lines.push("Faction tint: a faction presence colors the whole dungeon".to_string());
    }
    lines
}

/// Wall-clock seed for the content-plan roll (the plan PRNG is otherwise
/// deterministic for testing).
fn plan_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
}

fn build_seed_prompt(premise: Option<&str>, context: &str) -> Option<String> {
    let premise = premise.map(str::trim).filter(|value| !value.is_empty());
    let context = context.trim();
    match (premise, context.is_empty()) {
        (Some(premise), false) => Some(format!("{premise}\n\nContext: {context}")),
        (Some(premise), true) => Some(premise.to_string()),
        (None, false) => Some(format!(
            "Generate a self-contained 5-room dungeon.\n\nContext: {context}"
        )),
        (None, true) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_room_index_accepts_numbers_and_names() {
        assert_eq!(resolve_room_index("1"), Some(0));
        assert_eq!(resolve_room_index("5"), Some(4));
        assert_eq!(resolve_room_index("entrance"), Some(0));
        assert_eq!(resolve_room_index("Climax"), Some(3));
        assert_eq!(resolve_room_index("RESOLUTION"), Some(4));
    }

    #[test]
    fn resolve_room_index_rejects_out_of_range_and_unknown() {
        assert_eq!(resolve_room_index("0"), None);
        assert_eq!(resolve_room_index("6"), None);
        assert_eq!(resolve_room_index(""), None);
        assert_eq!(resolve_room_index("dungeon"), None);
    }
}
