use super::AiGenerationService;
use super::engine::*;
use super::reference::*;

use crate::repositories::{Database, GenerationRepository};
use crate::services::ollama_chat::{
    attempt_seed, build_chat_client, load_generation_config, post_chat_for_content,
};
use crate::utils::{estimate_tokens, normalize_name, normalize_unknown_text};
use runebound_models::DungeonBeat;
use runebound_models::dungeon_plan::DungeonContentPlan;
use runebound_models::utils::DUNGEON_FUNCTIONS;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DungeonBeatSeed {
    pub content_type: String,
    pub idea: String,
    #[serde(default)]
    pub player_goals: String,
    pub lever: String,
    #[serde(default)]
    pub loot: Option<String>,
    #[serde(default)]
    pub design_note: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DungeonSeed {
    pub name: String,
    #[serde(default)]
    pub location: String, // the single bounded place all five beats sit inside
    pub premise: String,
    pub beats: Vec<DungeonBeatSeed>,
}

impl DungeonSeed {
    /// Normalize narrative fields and the conditional loot line. `function` is
    /// assigned later in `to_beats`, not here, so the skeleton stays ours.
    fn normalize(&mut self) {
        self.name = normalize_name(&self.name);
        self.location = normalize_unknown_text(&self.location);
        self.premise = normalize_unknown_text(&self.premise);
        for beat in self.beats.iter_mut() {
            beat.content_type = normalize_unknown_text(&beat.content_type).to_ascii_lowercase();
            beat.idea = normalize_unknown_text(&beat.idea);
            beat.player_goals = normalize_unknown_text(&beat.player_goals);
            beat.lever = normalize_unknown_text(&beat.lever);
            beat.design_note = normalize_unknown_text(&beat.design_note);
            beat.loot = beat
                .loot
                .as_ref()
                .map(|loot| loot.trim().to_string())
                .filter(|loot| !loot.is_empty() && !loot.eq_ignore_ascii_case("none"));
        }
    }

    /// Convert to persistable beats, assigning the fixed function skeleton by
    /// position (beat 0 = Entrance … beat 4 = Resolution).
    pub fn to_beats(&self) -> Vec<DungeonBeat> {
        self.beats
            .iter()
            .enumerate()
            .map(|(i, beat)| DungeonBeat {
                function: DUNGEON_FUNCTIONS
                    .get(i)
                    .copied()
                    .unwrap_or("Beat")
                    .to_string(),
                content_type: beat.content_type.clone(),
                idea: beat.idea.clone(),
                player_goals: beat.player_goals.clone(),
                lever: beat.lever.clone(),
                loot: beat.loot.clone(),
                design_note: beat.design_note.clone(),
                // The plan's overlay/faction tint are stamped on afterward (the seed
                // doesn't carry them); see apply_plan_meta_to_beats.
                overlay: None,
                factions: false,
            })
            .collect()
    }
}

/// Pass 1 output: the prose the GM reviews before structuring (name + the one
/// bounded location + the one-to-two-paragraph story).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DungeonStory {
    pub name: String,
    #[serde(default)]
    pub location: String,
    pub story: String,
}

impl DungeonStory {
    fn normalize(&mut self) {
        self.name = normalize_name(&self.name);
        self.location = normalize_unknown_text(&self.location);
        self.story = self.story.trim().to_string();
    }
}

/// Pass 2 raw output: the spine plus five beats, each MISSING `content_type` —
/// that is injected from the plan, never requested from the model.
#[derive(Debug, Clone, serde::Deserialize)]
struct DungeonStructured {
    premise: String,
    beats: Vec<DungeonStructuredBeat>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct DungeonStructuredBeat {
    idea: String,
    #[serde(default)]
    player_goals: String,
    lever: String,
    #[serde(default)]
    loot: Option<String>,
    #[serde(default)]
    design_note: String,
}

fn describe_recent_dungeon_stories(payloads: Vec<String>) -> String {
    let names: Vec<String> = payloads
        .iter()
        .filter_map(|payload| serde_json::from_str::<DungeonStory>(payload).ok())
        .map(|story| story.name)
        .filter(|name| !name.trim().is_empty())
        .take(10)
        .collect();
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join("; ")
    }
}

/// Plain-language phrase for an anchor type, woven into the Pass-1 story so the
/// rolled content arrives without leaking the internal jargon.
fn anchor_story_phrase(content_type: &str) -> &'static str {
    match content_type {
        "combat" => "a hostile force or dangerous creature that must be fought or slipped past",
        "cache" => "a cache of treasure or reward waiting to be found",
        "forge" => "a forge, crucible, or workshop where something can be made or repaired",
        "puzzle" => {
            "a sealed way forward — a barred door or mechanism — that opens only once the party finds the right key or condition"
        }
        "offshoot" => {
            "an optional branching path: a side chamber, a hidden room, or a tempting dead end"
        }
        "sidekick" => {
            "a lone ally met here who joins the party and travels deeper with them through the rest of this place"
        }
        "oddity" => "a strange and significant object that is the very reason this place exists",
        "ability_check" => {
            "a feat of skill or nerve to get past — a climb, a leap, a steady hand, or a test of will"
        }
        _ => "something noteworthy",
    }
}

/// Mechanical meaning of an anchor type, given to Pass 2 so the card's idea
/// actually delivers that type's function. Also reused by the single-beat reroll,
/// which holds the rolled type fixed and only regenerates the prose.
pub(crate) fn anchor_mechanic(content_type: &str) -> &'static str {
    match content_type {
        "combat" => {
            "a fight; convey the enemy's tactics, behavior, and use of terrain, and NEVER name specific creatures (the GM picks them)"
        }
        "cache" => "a stash of loot or rewards",
        "forge" => {
            "a place to craft or repair magic items; the idea must involve that crafting or repair"
        }
        "puzzle" => {
            "a locked-door->key obstacle of one or more steps; never a riddle or logic puzzle"
        }
        "offshoot" => "an optional side passage, hidden room, or dead end off the main path",
        "sidekick" => {
            "a dungeon-only ally introduced here who joins the party and stays with them through the later beats, leaving only when the dungeon ends"
        }
        "oddity" => "the world-significant object that is the reason this dungeon exists",
        "ability_check" => {
            "an ability/skill check the party must pass — name the check (athletics, perception, persuasion, sleight of hand…) and what failure costs; not a riddle"
        }
        _ => "a noteworthy room",
    }
}

fn overlay_phrase(overlay_type: &str) -> &'static str {
    match overlay_type {
        "foreshadowing" => "a hint of something still to come, here or out in the wider campaign",
        "history" => "a piece of lore about this place, its people, or its makers",
        "map" => {
            "a glimpse of the surrounding world — a route, a landmark, or a link to somewhere else"
        }
        _ => "a telling detail",
    }
}

/// Plain-language layout for a topology, so its SHAPE can inform generation
/// without ever leaking the proper-noun name (e.g. "Foglio's Snail") into the
/// prose, where the model would otherwise reuse it as the dungeon's name.
fn topology_shape(topology: &str) -> Option<&'static str> {
    match topology {
        "The Railroad" => Some("a straight sequence of rooms, each leading to the next"),
        "The Moose" => Some("a short dead-end branch near the entrance off a longer main passage"),
        "The V for Vendetta" => {
            Some("two passages branching in opposite directions from the entrance")
        }
        "The Arrow" => Some("a three-way junction near the entrance"),
        "The Fauchard Fork" => Some("an early fork into one short path and one longer path"),
        "The Evil Mule" => Some("a branch that soon forks again into two"),
        "Foglio's Snail" => Some("two rooms deep, then a split into two hidden side rooms"),
        "The Paw" => Some("a hub that branches into three rooms"),
        "The Cross" => Some("a central hub with rooms opening off every side"),
        _ => None, // "none" / unknown — impose no spatial shape
    }
}

fn twist_directive(twist: &str) -> &'static str {
    match twist {
        "false_victory" => {
            "in the middle, hand the party an apparent win that then curdles — they think they've succeeded, then lose it"
        }
        "false_defeat" => "in the middle, stage an apparent loss the party then claws back from",
        _ => "play the arc straight — no fake-out in the middle",
    }
}

/// The numbered movement list for Pass 1: each beat's rolled anchor rendered as a
/// story ingredient, tied to its place in the arc.
fn pass1_elements_block(plan: &DungeonContentPlan) -> String {
    const LABELS: [&str; 5] = [
        "the way in",
        "the first turn inside",
        "where it costs them",
        "the peak",
        "the payoff",
    ];
    let mut out = String::new();
    for (i, anchor) in plan.anchors.iter().enumerate() {
        out.push_str(&format!(
            "  {}. ({}): {}\n",
            i + 1,
            LABELS[i],
            anchor_story_phrase(anchor)
        ));
    }
    out
}

/// The per-beat assignment block for Pass 2: fixed role + the GIVEN content type
/// and its mechanic + the loot rule, plus any overlay layer to fold in.
fn pass2_assignment_block(plan: &DungeonContentPlan) -> String {
    const ROLES: [&str; 5] = [
        "the way in (what stops a stray wanderer from getting through)",
        "the first obstacle inside (roleplay, a sealed way, or a trap)",
        "the cost, where the party PAYS",
        "the peak: a real confrontation, reversal, or revelation",
        "the payoff — a reward, a revelation, or humble pie, NOT another fight",
    ];
    const LOOT_RULES: [&str; 5] = [
        "Loot: null.",
        "Loot: null.",
        "Loot: null.",
        "Loot: only if it reads as the boss's hoard.",
        "Loot: REQUIRED — name a concrete reward the party claims here.",
    ];
    let mut out = String::new();
    for (i, anchor) in plan.anchors.iter().enumerate() {
        // A cache beat is a reward stash anywhere it lands: loot is mandatory.
        let loot_rule = if anchor == "cache" {
            "Loot: REQUIRED — name a concrete reward the party claims here."
        } else {
            LOOT_RULES[i]
        };
        out.push_str(&format!(
            "Beat {} — {}. Type: {} — {}. {}",
            i + 1,
            ROLES[i],
            anchor.to_uppercase(),
            anchor_mechanic(anchor),
            loot_rule,
        ));
        if let Some(overlay) = &plan.overlay
            && overlay.beat_index == i
        {
            out.push_str(&format!(
                " Also layer in {}: {}.",
                overlay.overlay_type,
                overlay_phrase(&overlay.overlay_type)
            ));
        }
        out.push('\n');
    }
    out
}

impl AiGenerationService {
    /// Pass 1 of dungeon generation: write the short story the GM reviews. The
    /// rolled content `plan` is fed in as plain-language story ingredients (never
    /// jargon), and the single-location anchor lives here because the story is
    /// where sprawl happens. `extra_prompt` carries the GM's optional reroll steer.
    #[allow(clippy::too_many_arguments)]
    pub async fn generate_dungeon_story(
        &self,
        plan: &DungeonContentPlan,
        premise: Option<String>,
        context: &str,
        tone: &str,
        twist: &str,
        topology: &str,
        extra_prompt: Option<&str>,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<DungeonStory>, String> {
        let (config, model) = load_generation_config()?;

        let premise = premise
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        let context = context.trim();
        let extra = extra_prompt
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());

        let reference_probe = format!("{} {}", premise.unwrap_or(""), context);
        let reference_context = build_reference_context(&config, reference_probe.trim()).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "dungeon_story", 12)
            .await?;
        let recent_context = describe_recent_dungeon_stories(recent_payloads);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(&reference_probe);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

        let schema = serde_json::json!({
            "type": "object",
            "required": ["name", "location", "story"],
            "additionalProperties": false,
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "location": { "type": "string", "minLength": 1 },
                "story": { "type": "string", "minLength": 1 }
            }
        });

        let premise_directive = match premise {
            Some(value) => format!("Build the story to honor this premise: \"{value}\"."),
            None => {
                "Invent a small, self-contained story that needs nothing outside this one place."
                    .to_string()
            }
        };
        let context_directive = if context.is_empty() {
            String::new()
        } else {
            format!("Weave in these GM-supplied details where natural: {context}.")
        };
        let faction_directive = if plan.factions {
            "Rival factions contest this place; thread their conflict through the story, and make the peak a confrontation between forces rather than a lone monster. ".to_string()
        } else {
            String::new()
        };
        let overlay_directive = match &plan.overlay {
            Some(overlay) => format!(
                "In movement {}, also plant {}, lightly — a layer on the scene, not its center. ",
                overlay.beat_index + 1,
                overlay_phrase(&overlay.overlay_type)
            ),
            None => String::new(),
        };
        let topology_directive = match topology_shape(topology) {
            Some(shape) => {
                format!(
                    "The space is shaped like {shape}; let that guide how the party moves deeper. "
                )
            }
            None => String::new(),
        };
        let steer_directive = match extra {
            Some(value) => format!("The GM asked for this in the retold version: {value}. "),
            None => String::new(),
        };
        let sidekick_directive = match plan.anchors.iter().position(|a| a == "sidekick") {
            Some(idx) => format!(
                "The ally introduced in movement {} is a companion, not a place: they join the party and travel with them through the movements that follow, until the dungeon ends — keep them present in those later movements rather than forgetting them after their first scene. ",
                idx + 1
            ),
            None => String::new(),
        };

        let elements = pass1_elements_block(plan);

        let system_prompt = format!(
            "You are a master storyteller seeding a dungeon for a tabletop game master. Your goal is a COMPLETE, self-contained micro-story in two short paragraphs — a real tale that runs from a clear beginning to a definite END, not a fragment, a mood piece, or a description of a place. Things must HAPPEN and someone must ACT; carry the tale all the way to its ending and never trail off in atmosphere. A GM should read it in fifteen seconds and see the whole shape of an adventure. The north star is SPECIFIC BUT UNRESOLVED: concrete, evocative sparks that raise questions, even as the tale itself reaches a complete arc.\n\n\
ONE LOCATION. The whole story happens inside a single bounded place the party enters and moves DEEPER into — e.g. \"a drowned bell-foundry\", \"a hijacked customs house\". Name that place. They never travel to another region, town, or building; they go further in, not elsewhere. Keep the cast and threats consistent from first line to last.\n\n\
Move through five movements in order — a setup, an inciting turn, rising tension, a peak, and a resolution — and actually REACH the fifth (the ending); do not stop after the setup or the descent. Do NOT label the parts; let it read as two flowing paragraphs, roughly six to ten sentences total. Pace the movements so the confrontation lands at the FOURTH movement (the peak), never earlier. Each element below belongs to exactly ONE movement — build that movement around it and keep it out of the others:\n\n{elements}\n\
Tone: {tone} — let it color the whole arc. Twist: {twist}. {faction_directive}{sidekick_directive}{overlay_directive}{topology_directive}{steer_directive}\n\n\
{premise_directive} {context_directive}\n\n\
Avoid retelling these recent stories: {recent}.{reference}\n\n\
Return only JSON: name (a short evocative title), location (the one place, a short phrase), and story (the complete two-paragraph tale, beginning to end).",
            elements = elements,
            tone = tone,
            twist = twist_directive(twist),
            faction_directive = faction_directive,
            sidekick_directive = sidekick_directive,
            overlay_directive = overlay_directive,
            topology_directive = topology_directive,
            steer_directive = steer_directive,
            premise_directive = premise_directive,
            context_directive = context_directive,
            recent = recent_context,
            reference = if reference_context.system_context.is_empty() {
                String::new()
            } else {
                format!("\n\n{}", reference_context.system_context)
            },
        );

        let user_prompt = match premise {
            Some(value) => value.to_string(),
            None => "Write the story.".to_string(),
        };

        let (client, url) = build_chat_client(&config)?;

        for attempt in 0..5 {
            let run_seed = attempt_seed(attempt);
            let repair_note = if attempt == 0 {
                ""
            } else {
                " Previous response was invalid. Return only valid JSON matching the schema."
            };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.05, "top_p": 0.92, "repeat_penalty": 1.1, "seed": run_seed, "num_ctx": config.ollama.num_ctx },
                "messages": [
                    { "role": "system", "content": format!("{system_prompt}{repair_note}") },
                    { "role": "user", "content": user_prompt }
                ]
            });

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: Result<DungeonStory, _> = serde_json::from_str(&content);
            let Ok(mut story) = parsed else { continue };
            story.normalize();
            if story.name.is_empty() || story.location == "Unknown" || story.story.is_empty() {
                continue;
            }

            let serialized = serde_json::to_string(&story).map_err(|err| err.to_string())?;
            generation_repo
                .insert(database, "dungeon_story", None, &serialized)
                .await?;

            return Ok(SeedGeneration {
                seed: story,
                notice,
            });
        }

        Err("failed to generate a valid dungeon story from ollama".to_string())
    }
    /// Pass 2 of dungeon generation: structure the LOCKED story into the five beat
    /// cards. Extractive — the model maps the story it is given, applies the field
    /// leashes, and writes a one-line spine. The per-beat `content_type` is NOT
    /// requested; it is injected from the deterministic `plan` so the tag can never
    /// disagree with the content. `function` is assigned by position in `to_beats`.
    #[allow(clippy::too_many_arguments)]
    pub async fn structure_dungeon_story(
        &self,
        plan: &DungeonContentPlan,
        story: &DungeonStory,
        tone: &str,
        twist: &str,
        topology: &str,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<DungeonSeed>, String> {
        let (config, model) = load_generation_config()?;

        let beat_schema = serde_json::json!({
            "type": "object",
            "required": ["idea", "player_goals", "lever", "design_note"],
            "additionalProperties": false,
            "properties": {
                "idea": { "type": "string", "minLength": 1 },
                "player_goals": { "type": "string", "minLength": 1 },
                "lever": { "type": "string", "minLength": 1 },
                "loot": { "type": ["string", "null"] },
                "design_note": { "type": "string", "minLength": 1 }
            }
        });
        let schema = serde_json::json!({
            "type": "object",
            "required": ["premise", "beats"],
            "additionalProperties": false,
            "properties": {
                "premise": { "type": "string", "minLength": 1 },
                "beats": { "type": "array", "minItems": 5, "maxItems": 5, "items": beat_schema }
            }
        });

        let assignment = pass2_assignment_block(plan);
        let faction_note = if plan.factions {
            "These beats sit inside a faction struggle; let it tint the relevant beats and render the peak as a confrontation between forces. ".to_string()
        } else {
            String::new()
        };
        let topology_note = match topology_shape(topology) {
            Some(shape) => {
                format!(
                    "Spatial layout: {shape}; let it inform how the beats connect (especially whether the Setback loops the party back toward the entrance). "
                )
            }
            None => String::new(),
        };
        let sidekick_note = match plan.anchors.iter().position(|a| a == "sidekick") {
            Some(idx) => format!(
                "The sidekick beat (beat {}) introduces a companion who then accompanies the party: where the story shows them, let the idea or lever of the beats AFTER it involve that ally rather than dropping them after their introduction. ",
                idx + 1
            ),
            None => String::new(),
        };

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&assignment)
            + estimate_tokens(&story.story);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

        let system_prompt = format!(
            "You are structuring a finished story into a game master's index cards. The story below is LOCKED — do not invent new events, places, characters, or items; only express what is already there. The north star is SPECIFIC BUT UNRESOLVED: each field is a concrete spark that never states the final answer.\n\n\
The story has five movements in order. Produce exactly five beats in the same order; beat N renders movement N. SCOPE EACH BEAT TO ITSELF: every field describes ONLY what happens in that one beat — never summarize the whole dungeon in a single beat, never name the final confrontation or ending before its own beat, and do not let beat 1 preview the climax. For each beat write four fields:\n\
- idea: 1-2 sentences — what happens in THIS beat only.\n\
- player_goals: 1 sentence — the clear, concrete goal for the players in THIS beat: what they must learn, do, reach, or overcome to complete it (not the goal of the whole dungeon).\n\
- lever: ONE complication, question, or hook the GM can pull, in 1-2 sentences.\n\
- loot: a conditional reward line, OR null (see each beat's rule below).\n\
- design_note: 1 sentence to the GM (out of fiction) — how this beat fits the overall dungeon and story: what it sets up, pays off, or escalates.\n\n\
Each beat has a fixed role and content type, written here. Honor them EXACTLY — every beat must deliver its listed content type's mechanic, even where that movement of the story is brief: lead the idea with that mechanic, recasting the story's own props (a chain, a hook, a ledger) to serve it. Never change a beat's type. A beat typed combat MUST stage an actual fight (convey tactics and behavior) — never render a combat beat as a choice, a conversation, or a quiet decision. A non-combat beat must NOT be turned into a fight. The FINAL beat is the payoff — a reward, a revelation, or humble pie — NOT a second battle:\n\n{assignment}\n\
{faction_note}{sidekick_note}{topology_note}Tone: {tone}. Twist shape: {twist}.\n\n\
Also produce premise: a single-line spine summarizing the whole dungeon (one sentence; specific but unresolved).\n\n\
Keep every field tight — 1-2 sentences; a paragraph of boxed text is over-generating. Return only JSON: premise, and the five beats (idea, player_goals, lever, loot, design_note) in order.",
            assignment = assignment,
            faction_note = faction_note,
            sidekick_note = sidekick_note,
            topology_note = topology_note,
            tone = tone,
            twist = twist,
        );

        let user_prompt = format!(
            "Title: {}\nLocation: {}\n\nStory:\n{}",
            story.name, story.location, story.story
        );

        let (client, url) = build_chat_client(&config)?;

        for attempt in 0..5 {
            let run_seed = attempt_seed(attempt);
            let repair_note = if attempt == 0 {
                ""
            } else {
                " Previous response was invalid. Return only valid JSON matching the schema with exactly five beats."
            };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 0.7, "top_p": 0.9, "repeat_penalty": 1.1, "seed": run_seed, "num_ctx": config.ollama.num_ctx },
                "messages": [
                    { "role": "system", "content": format!("{system_prompt}{repair_note}") },
                    { "role": "user", "content": user_prompt }
                ]
            });

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: Result<DungeonStructured, _> = serde_json::from_str(&content);
            let Ok(structured) = parsed else { continue };
            if structured.beats.len() != DUNGEON_FUNCTIONS.len() {
                continue;
            }

            // Inject content_type per beat from the deterministic plan; carry
            // name/location from Pass 1; take the spine from Pass 2.
            let beats = structured
                .beats
                .into_iter()
                .enumerate()
                .map(|(i, beat)| DungeonBeatSeed {
                    content_type: plan.anchors[i].clone(),
                    idea: beat.idea,
                    player_goals: beat.player_goals,
                    lever: beat.lever,
                    loot: beat.loot,
                    design_note: beat.design_note,
                })
                .collect();
            let mut seed = DungeonSeed {
                name: story.name.clone(),
                location: story.location.clone(),
                premise: structured.premise,
                beats,
            };
            seed.normalize();

            return Ok(SeedGeneration { seed, notice });
        }

        Err("failed to structure the dungeon story into cards from ollama".to_string())
    }
}
