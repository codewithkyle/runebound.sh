//! The guided `create dungeon` flow (steps A–E). Modeled on the bespoke setup
//! wizard (`core::command::try_execute_onboarding`): a small step counter with
//! typed answer fields, intercepted before registry dispatch while active. On the
//! final answer it generates the whole dungeon and opens it as an editable draft.

use dnd_core::npc::slugify;
use runebound_models::output::{
    OutputDoc, doc, heading, list, paragraph_text, text_node,
};
use runebound_models::dungeon_plan::roll_dungeon_content_plan;
use runebound_models::utils::{DUNGEON_TONES, DUNGEON_TOPOLOGIES, DUNGEON_TWISTS, make_entity_id};

use crate::app_state::{AppState, DungeonCreationFlow, DungeonDraftSession};
use crate::commands::{dungeon_event_from_draft, dungeon_summary_text};
use crate::entities::EntityKind;
use crate::entities::common::{
    command_message_response_with_doc, command_response_with_event, CommandResult,
};
use crate::services::ai_generation::{AiGenerationService, DungeonStory, SeedGeneration};
use crate::utils::{normalize_unknown_text, prepend_notice};

/// Marker line in the Step E prompt. The frontend spinner heuristic
/// (`detectDungeonTopologyPrompt`) keys off this so the completing answer shows a
/// "generating dungeon" spinner.
pub const STEP_E_MARKER: &str = "Step 5 of 5 — Topology";

/// Entry point: `create dungeon` activates the flow and returns the Step A prompt.
pub async fn start_dungeon_flow(state: &AppState) -> CommandResult {
    {
        let mut flow = state.dungeon_flow.lock().await;
        *flow = DungeonCreationFlow {
            active: true,
            step: 1,
            ..Default::default()
        };
    }
    command_message_response_with_doc(step_a_text_plain(), step_a_doc())
}

/// Intercepted before registry dispatch while the flow is active. Validates the
/// raw line against the current step, advances, and on Step E generates + opens
/// the draft. Handles `cancel` explicitly (the desktop cancel handler never runs
/// during the flow — same invariant as the setup wizard).
pub async fn try_execute_dungeon_flow(line: &str, state: &AppState) -> CommandResult {
    let trimmed = line.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "cancel" || lowered == "cancel dungeon" {
        reset_dungeon_flow(state).await;
        return command_message_response_with_doc(
            "dungeon creation cancelled.",
            doc().with_block(paragraph_text("Dungeon creation cancelled.")),
        );
    }

    let step = {
        let flow = state.dungeon_flow.lock().await;
        flow.step
    };

    match step {
        1 => handle_step_a(trimmed, state).await,
        2 => handle_step_b(trimmed, state).await,
        3 => handle_step_c(trimmed, state).await,
        4 => handle_step_d(trimmed, state).await,
        5 => handle_step_e(trimmed, state).await,
        6 => handle_step_f(trimmed, state).await,
        _ => {
            // Defensive: an unknown step shouldn't happen, but never trap the user.
            reset_dungeon_flow(state).await;
            command_message_response_with_doc(
                "dungeon flow reset.",
                doc().with_block(paragraph_text("Dungeon flow reset; run create dungeon again.")),
            )
        }
    }
}

pub async fn reset_dungeon_flow(state: &AppState) {
    let mut flow = state.dungeon_flow.lock().await;
    *flow = DungeonCreationFlow::default();
}

// ---------------------------------------------------------------------------
// Step A — Premise
// ---------------------------------------------------------------------------

fn step_a_doc() -> OutputDoc {
    doc()
        .with_block(heading(2, "Create Dungeon — Step 1 of 5 — Premise"))
        .with_block(paragraph_text(
            "Enter a one-line premise, or type `generate` to have the oracle invent one.",
        ))
}

fn step_a_text_plain() -> String {
    "Step 1 of 5 — Premise: enter a one-line premise, or type `generate`.".to_string()
}

async fn handle_step_a(trimmed: &str, state: &AppState) -> CommandResult {
    if trimmed.is_empty() {
        return command_message_response_with_doc(step_a_text_plain(), step_a_doc());
    }
    {
        let mut flow = state.dungeon_flow.lock().await;
        flow.premise = if trimmed.eq_ignore_ascii_case("generate") {
            None
        } else {
            Some(trimmed.to_string())
        };
        flow.step = 2;
    }
    command_message_response_with_doc(step_b_text_plain(), step_b_doc())
}

// ---------------------------------------------------------------------------
// Step B — Tone
// ---------------------------------------------------------------------------

fn step_b_doc() -> OutputDoc {
    menu_doc(
        "Create Dungeon — Step 2 of 5 — Tone",
        "Choose the overall emotional polarity:",
        &["1: Tragedy", "2: Comedy"],
    )
}

fn step_b_text_plain() -> String {
    "Step 2 of 5 — Tone: 1: Tragedy   2: Comedy".to_string()
}

async fn handle_step_b(trimmed: &str, state: &AppState) -> CommandResult {
    let tone = match trimmed {
        "1" => "tragedy",
        "2" => "comedy",
        _ => return command_message_response_with_doc(step_b_text_plain(), step_b_doc()),
    };
    debug_assert!(DUNGEON_TONES.contains(&tone));
    {
        let mut flow = state.dungeon_flow.lock().await;
        flow.tone = Some(tone.to_string());
        flow.step = 3;
    }
    command_message_response_with_doc(step_c_text_plain(), step_c_doc())
}

// ---------------------------------------------------------------------------
// Step C — Twist
// ---------------------------------------------------------------------------

fn step_c_doc() -> OutputDoc {
    menu_doc(
        "Create Dungeon — Step 3 of 5 — Twist",
        "Choose the shape of the middle beats:",
        &["1: False victory", "2: False defeat", "3: Neither"],
    )
}

fn step_c_text_plain() -> String {
    "Step 3 of 5 — Twist: 1: False victory   2: False defeat   3: Neither".to_string()
}

async fn handle_step_c(trimmed: &str, state: &AppState) -> CommandResult {
    let twist = match trimmed {
        "1" => "false_victory",
        "2" => "false_defeat",
        "3" => "neither",
        _ => return command_message_response_with_doc(step_c_text_plain(), step_c_doc()),
    };
    debug_assert!(DUNGEON_TWISTS.contains(&twist));
    {
        let mut flow = state.dungeon_flow.lock().await;
        flow.twist = Some(twist.to_string());
        flow.step = 4;
    }
    command_message_response_with_doc(step_d_text_plain(), step_d_doc())
}

// ---------------------------------------------------------------------------
// Step D — Context
// ---------------------------------------------------------------------------

fn step_d_doc() -> OutputDoc {
    doc()
        .with_block(heading(2, "Create Dungeon — Step 4 of 5 — Context"))
        .with_block(paragraph_text(
            "Add references/constraints to seed the oracle (or type `skip`). You may include @references to vault documents.",
        ))
}

fn step_d_text_plain() -> String {
    "Step 4 of 5 — Context: add references/constraints, or `skip`.".to_string()
}

async fn handle_step_d(trimmed: &str, state: &AppState) -> CommandResult {
    {
        let mut flow = state.dungeon_flow.lock().await;
        flow.context = if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("skip") {
            String::new()
        } else {
            trimmed.to_string()
        };
        flow.step = 5;
    }
    command_message_response_with_doc(step_e_text_plain(), step_e_doc())
}

// ---------------------------------------------------------------------------
// Step E — Topology
// ---------------------------------------------------------------------------

fn step_e_doc() -> OutputDoc {
    // DUNGEON_TOPOLOGIES[0] is "none"; render it as option 0, the rest 1..=9.
    let mut options: Vec<String> = vec!["0: None (lay it out freely)".to_string()];
    for (i, name) in DUNGEON_TOPOLOGIES.iter().enumerate().skip(1) {
        options.push(format!("{i}: {name}"));
    }
    let option_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
    menu_doc(
        STEP_E_MARKER,
        "Pick one of the nine forms, or 0 for none:",
        &option_refs,
    )
}

fn step_e_text_plain() -> String {
    format!("{STEP_E_MARKER}: 0: None  1: The Railroad … 9: The Cross")
}

async fn handle_step_e(trimmed: &str, state: &AppState) -> CommandResult {
    let index: usize = match trimmed.parse() {
        Ok(value) if value < DUNGEON_TOPOLOGIES.len() => value,
        _ => return command_message_response_with_doc(step_e_text_plain(), step_e_doc()),
    };
    let topology = DUNGEON_TOPOLOGIES[index].to_string();

    // Snapshot the collected answers, store topology, then roll + write the story.
    let (premise, context, tone, twist) = {
        let mut flow = state.dungeon_flow.lock().await;
        flow.topology = Some(topology.clone());
        (
            flow.premise.clone(),
            flow.context.clone(),
            flow.tone.clone().unwrap_or_else(|| "tragedy".to_string()),
            flow.twist.clone().unwrap_or_else(|| "neither".to_string()),
        )
    };

    generate_and_review_story(state, premise, &context, &tone, &twist, &topology, None).await
}

/// Roll a fresh content plan, run Pass 1, stash it on the flow, and show the story
/// for review (the `continue` / `reroll` screen). Shared by Step E and `reroll`.
async fn generate_and_review_story(
    state: &AppState,
    premise: Option<String>,
    context: &str,
    tone: &str,
    twist: &str,
    topology: &str,
    extra_prompt: Option<&str>,
) -> CommandResult {
    let plan = roll_dungeon_content_plan(plan_seed());

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
            context,
            tone,
            twist,
            topology,
            extra_prompt,
            &state.workspace_root,
            database.as_ref(),
            generation_repo.as_ref(),
        )
        .await?;

    {
        let mut flow = state.dungeon_flow.lock().await;
        flow.plan = Some(plan);
        flow.story_name = Some(story.name.clone());
        flow.story_location = Some(story.location.clone());
        flow.story_text = Some(story.story.clone());
        flow.step = 6;
    }

    command_message_response_with_doc(
        prepend_notice(notice.clone(), review_text_plain(&story)),
        review_doc(&story, &notice),
    )
}

// ---------------------------------------------------------------------------
// Step F — Story review (continue / reroll)
// ---------------------------------------------------------------------------

async fn handle_step_f(trimmed: &str, state: &AppState) -> CommandResult {
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest = parts.next().unwrap_or("").trim();

    match cmd.as_str() {
        "continue" | "accept" => finalize_dungeon(state).await,
        "reroll" | "redo" => {
            reroll_story(state, (!rest.is_empty()).then_some(rest)).await
        }
        _ => match current_review_story(state).await {
            Some(story) => command_message_response_with_doc(
                review_text_plain(&story),
                review_doc(&story, &None),
            ),
            None => {
                reset_dungeon_flow(state).await;
                command_message_response_with_doc(
                    "dungeon flow reset.",
                    doc().with_block(paragraph_text(
                        "Dungeon flow reset; run create dungeon again.",
                    )),
                )
            }
        },
    }
}

/// `reroll [hint]` at the review screen: roll a brand-new plan and story (variety
/// across rerolls comes from re-rolling which type fills which beat), optionally
/// steered by the GM's hint.
async fn reroll_story(state: &AppState, extra: Option<&str>) -> CommandResult {
    let (premise, context, tone, twist, topology) = {
        let flow = state.dungeon_flow.lock().await;
        (
            flow.premise.clone(),
            flow.context.clone(),
            flow.tone.clone().unwrap_or_else(|| "tragedy".to_string()),
            flow.twist.clone().unwrap_or_else(|| "neither".to_string()),
            flow.topology.clone().unwrap_or_else(|| "none".to_string()),
        )
    };
    generate_and_review_story(state, premise, &context, &tone, &twist, &topology, extra).await
}

/// `continue` at the review screen: structure the locked story (Pass 2), assemble
/// the seed, and open the editable draft — the same hand-off Step E used to do.
async fn finalize_dungeon(state: &AppState) -> CommandResult {
    let (plan, story, premise, context, tone, twist, topology) = {
        let flow = state.dungeon_flow.lock().await;
        let story = match (flow.story_name.clone(), flow.story_text.clone()) {
            (Some(name), Some(text)) => Some(DungeonStory {
                name,
                location: flow.story_location.clone().unwrap_or_default(),
                story: text,
            }),
            _ => None,
        };
        (
            flow.plan.clone(),
            story,
            flow.premise.clone(),
            flow.context.clone(),
            flow.tone.clone().unwrap_or_else(|| "tragedy".to_string()),
            flow.twist.clone().unwrap_or_else(|| "neither".to_string()),
            flow.topology.clone().unwrap_or_else(|| "none".to_string()),
        )
    };

    let (Some(plan), Some(story)) = (plan, story) else {
        reset_dungeon_flow(state).await;
        return command_message_response_with_doc(
            "dungeon flow reset.",
            doc().with_block(paragraph_text("Dungeon flow reset; run create dungeon again.")),
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
    let seed_prompt = build_seed_prompt(premise.as_deref(), &context);

    let draft = DungeonDraftSession {
        id: make_entity_id("dungeon"),
        seed_prompt,
        name: seed.name.trim().to_string(),
        slug: slugify(seed.name.trim()),
        vault_path: String::new(),
        location: normalize_unknown_text(&seed.location),
        premise: normalize_unknown_text(&seed.premise),
        topology,
        tone,
        twist,
        beats: seed.into_beats(),
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
    reset_dungeon_flow(state).await;

    command_response_with_event(
        prepend_notice(notice, dungeon_summary_text(&draft)),
        dungeon_event_from_draft(&draft),
    )
}

async fn current_review_story(state: &AppState) -> Option<DungeonStory> {
    let flow = state.dungeon_flow.lock().await;
    Some(DungeonStory {
        name: flow.story_name.clone()?,
        location: flow.story_location.clone().unwrap_or_default(),
        story: flow.story_text.clone()?,
    })
}

fn review_text_plain(story: &DungeonStory) -> String {
    format!(
        "{} — {}\n\n{}\n\nType `continue` to build the cards, `reroll [hint]` for a new story, or `cancel`.",
        story.name, story.location, story.story
    )
}

fn review_doc(story: &DungeonStory, notice: &Option<String>) -> OutputDoc {
    let mut out = doc();
    if let Some(note) = notice {
        out = out.with_block(paragraph_text(note.clone()));
    }
    out.with_block(heading(2, format!("{} — {}", story.name, story.location)))
        .with_block(paragraph_text(story.story.clone()))
        .with_block(paragraph_text(
            "Type `continue` to build the five cards, `reroll [hint]` for a new story, or `cancel` to stop.",
        ))
}

/// Wall-clock seed for the content-plan roll, mirroring how ollama retry seeds are
/// derived (the plan PRNG is otherwise deterministic for testing).
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

fn menu_doc(title: &str, intro: &str, options: &[&str]) -> OutputDoc {
    let items: Vec<Vec<_>> = options
        .iter()
        .map(|option| vec![text_node(option.to_string())])
        .collect();
    doc()
        .with_block(heading(2, title.to_string()))
        .with_block(paragraph_text(intro.to_string()))
        .with_block(list(items))
}
