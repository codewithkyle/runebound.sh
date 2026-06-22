use super::AiGenerationService;
use super::engine::*;
use super::reference::*;

use crate::repositories::{Database, GenerationRepository};
use crate::services::ollama_chat::{OllamaChatClient, detail_directive, load_generation_config};
use crate::utils::{
    estimate_tokens, normalize_exports, normalize_location_danger_level, normalize_location_seed,
    normalize_name, normalize_unknown_text, validate_location_details, validate_location_prose,
};
use runebound_models::utils::{LOCATION_DANGER_LEVELS, LOCATION_KIND_TYPES};
use std::collections::HashSet;

/// Which structured location branch a kind routes to. Drives the per-kind schema
/// shape and prompt in [`AiGenerationService::generate_location_seed_for_wizard`].
/// (Only freeform custom kinds are *not* structured — they stay on the one-shot path.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationBranch {
    Settlement,
    Site,
    Hideout,
    /// A faction's public headquarters. Exports suppressed, `authority` locked to
    /// the linked faction, `danger_level` LLM-derived (a public hall's danger is
    /// usually incidental, like a settlement's).
    Guildhall,
}

/// Map a GM-locked kind to its branch. Only the structured kinds reach the wizard
/// generation method; anything else falls back to Settlement defensively.
pub fn location_branch(kind_type: &str) -> LocationBranch {
    match kind_type {
        "ruin" | "landmark" | "wilderness" => LocationBranch::Site,
        "hideout" => LocationBranch::Hideout,
        "guildhall" => LocationBranch::Guildhall,
        _ => LocationBranch::Settlement,
    }
}

/// Under-`locations/` subfolder for a structured wizard kind, or `None` for
/// other/freeform/unknown (stays flat). Unlike [`location_branch`], `other` must map
/// to `None` rather than a default branch, so match the kind strings directly. Only
/// shapes the readable `.md` vault_path — the TOML store and DB projection stay flat.
pub fn location_subfolder(kind_type: &str) -> Option<&'static str> {
    match kind_type {
        "hamlet" | "town" | "city" => Some("settlements"),
        "ruin" | "landmark" | "wilderness" => Some("sites"),
        "hideout" => Some("hideouts"),
        "guildhall" => Some("guildhalls"),
        _ => None, // other / freeform / unknown -> flat
    }
}

/// Full relative dir for a NEW location save: `locations/<sub>` for a structured
/// wizard kind, or flat `locations` otherwise.
pub fn location_dir_for_kind(base: &str, kind_type: &str) -> String {
    match location_subfolder(kind_type) {
        Some(sub) => format!("{base}/{sub}"),
        None => base.to_string(),
    }
}

/// The GM's locked wizard answers, flattened into a borrow-friendly struct the
/// wizard fills and passes to generation. Keeps `ai_generation.rs` free of the
/// wizard's own accumulator type.
#[derive(Debug, Clone, Default)]
pub struct LocationWizardInputs {
    pub kind_type: String,
    pub kind_custom: Option<String>,
    // Settlement (Q-A…Q-D)
    pub control: Option<String>,
    pub resources: Option<String>,
    pub export_mode: Option<String>,
    // Site (Q-S1…Q-S3)
    pub site_focus: Option<String>,
    pub site_draw: Option<String>,
    // The deity a "holy site" draw is devoted to. Emitted as `@gods/<name>` so the
    // god's vault note grounds the prose (mirrors the faction's god link). Grounding
    // only — never generated.
    pub site_god: Option<String>,
    // Hideout (Q-H1…Q-H4)
    pub base_owner: Option<String>,
    pub base_protection: Option<String>,
    pub base_purpose: Option<String>,
    // GM-locked danger for Site + Hideout (Q-S2 / Q-H3)
    pub danger_lock: Option<String>,
    // Guildhall (faction-locked public HQ): its public-facing function, and the
    // existing location it stands within (or a free-typed place name).
    pub public_role: Option<String>,
    pub location_anchor: Option<String>,
    // The anchor location's under-`locations/` subfolder (e.g. "settlements"), so the
    // `@locations/<sub>/<anchor>` seed resolves to the right path-keyed note. Empty/None
    // for a flat note or a free-typed place; falls back to the name-only `@locations/<anchor>`.
    pub location_anchor_sub: Option<String>,
    // Shared optional map anchor (Q-D / Q-S4 / Q-H5)
    pub geography: Option<String>,
    // A linked faction's canonical name (read-only), forces `authority`.
    pub faction_name: Option<String>,
    // Optional reroll steer from the review screen.
    pub hint: Option<String>,
}

fn geography_clause(geography: &Option<String>) -> String {
    match opt_clause(geography) {
        Some(geo) => format!(" Ground it on the GM's map: {geo}."),
        None => String::new(),
    }
}

/// The fixed-vs-unspecified danger directive for Site/Hideout. The level itself is
/// injected after generation; the prompt only asks the model to write its *source*.
fn locked_danger_clause(danger: &Option<String>) -> String {
    match danger.as_deref().unwrap_or("Unknown") {
        "Unknown" => " The danger level is deliberately left open; let any tension emerge naturally without forcing a severity.".to_string(),
        level => format!(
            " The danger level is fixed at '{level}'; write the SOURCE of that danger and how it manifests in current_tension and history_background, but do not contradict that level."
        ),
    }
}

const LOCATION_PROSE_LEASH: &str = " visual_description must be 1-3 sentences, history_background 2-5 sentences, current_tension 1-2 sentences, and tone 2-5 words.";

/// Build the branch-specific system prompt that embeds the GM's locked answers.
/// The recent-seed avoidance, repair note, reference block, and detail directive
/// are appended by the caller's closure.
fn wizard_location_system_prompt(inputs: &LocationWizardInputs, branch: LocationBranch) -> String {
    let kind = &inputs.kind_type;
    let geo = geography_clause(&inputs.geography);
    match branch {
        LocationBranch::Settlement => {
            let control = opt_clause(&inputs.control).unwrap_or("a single ruler or house");
            let resources = opt_clause(&inputs.resources)
                .unwrap_or("whatever the surrounding land naturally provides");
            let export_mode = opt_clause(&inputs.export_mode).unwrap_or("mixed");
            let exports_clause = if export_mode == "none" {
                " This settlement exports nothing — return an empty exports list (a consuming frontier or garrison town that lives off imports, not a producer).".to_string()
            } else {
                format!(
                    " Its exports are {export_mode} — produce a 1-3 item exports list consistent with that: raw → ship the resource roughly as-is; refined → the processed or finished good made from it; mixed → some of both."
                )
            };
            format!(
                "You generate one usable D&D settlement seed (a {kind}) for a game master. Return only JSON with fields name, visual_description, history_background, exports, tone, authority, danger_level, current_tension. This settlement's power structure: {control}. The authority field must describe exactly that power structure — including any absence of central rule — and must not invent a governing body that the power structure rules out. Its natural resources are: {resources}.{exports_clause}{geo} danger_level must be one of: {danger}.{leash}",
                danger = LOCATION_DANGER_LEVELS.join(", "),
                leash = LOCATION_PROSE_LEASH,
            )
        }
        LocationBranch::Site => {
            let focus = match inputs.site_focus.as_deref() {
                Some("past") => {
                    "what this place WAS — weight history_background heavily and keep the present quiet"
                }
                Some("present") => {
                    "what is HERE NOW — weight the current occupant and current_tension, keeping history light"
                }
                _ => "a balance of what it was and what is here now",
            };
            let draw = match opt_clause(&inputs.site_draw) {
                Some(draw) => format!(" The reason players are drawn here: {draw}."),
                None => String::new(),
            };
            let holy = match opt_clause(&inputs.site_god) {
                Some(god) => format!(
                    " This place is holy to {god}; steep its iconography, history_background, and current_tension in that deity's domain and worship, name its presiding clergy or sacred guardian in authority, and keep any name consistent with the referenced metadata about that god."
                ),
                None => String::new(),
            };
            let danger = locked_danger_clause(&inputs.danger_lock);
            format!(
                "You generate one usable D&D site seed (a {kind}) for a game master — a place the party stumbles upon, NOT a settlement. Return only JSON with fields name, visual_description, history_background, tone, authority, current_tension. Weight the writing toward {focus}.{draw}{holy}{geo} Do not invent rulers, governments, or exports; authority should name the lone occupant or guardian of the place, or 'Unknown' if it stands empty.{danger}{leash}",
                leash = LOCATION_PROSE_LEASH,
            )
        }
        LocationBranch::Hideout => {
            let owner = opt_clause(&inputs.base_owner).unwrap_or("a single operator");
            let protection = opt_clause(&inputs.base_protection).unwrap_or("secrecy");
            let purpose = opt_clause(&inputs.base_purpose).unwrap_or("refuge");
            let danger = locked_danger_clause(&inputs.danger_lock);
            format!(
                "You generate one usable D&D hideout seed (a {kind}) for a game master — someone's deliberately hidden, actively occupied base, NOT a ruin. Return only JSON with fields name, visual_description, history_background, tone, authority, current_tension. The base is owned by {owner}; the authority field must name that owner. It is protected by {protection} and exists for {purpose} — let that drive its defenses and how players might find it. Write it present-tense and do not invent exports.{geo}{danger}{leash}",
                leash = LOCATION_PROSE_LEASH,
            )
        }
        LocationBranch::Guildhall => {
            let faction = opt_clause(&inputs.faction_name).unwrap_or("an established organization");
            let role = match opt_clause(&inputs.public_role) {
                Some(role) => format!(" It functions publicly as {role}."),
                None => String::new(),
            };
            // The hall stands within an existing place (Q-G3); ground it there.
            let anchor = match opt_clause(&inputs.location_anchor) {
                Some(place) => {
                    format!(" This hall stands within {place}; ground it in that place.")
                }
                None => String::new(),
            };
            format!(
                "You generate one usable D&D guildhall seed (a {kind}) for a game master — the PUBLIC headquarters of an established organization, NOT a settlement and NOT a hidden base. Return only JSON with fields name, visual_description, history_background, tone, authority, danger_level, current_tension. This hall is the public seat of {faction}; the authority field must name {faction}, and the visual_description, history_background, tone, and current_tension must reflect that organization's identity, methods, and goals.{role}{anchor} Do not invent exports or trade goods. danger_level must be one of: {danger} — a public hall's danger is usually low unless the organization courts it.{leash}",
                danger = LOCATION_DANGER_LEVELS.join(", "),
                leash = LOCATION_PROSE_LEASH,
            )
        }
    }
}

fn wizard_location_schema(branch: LocationBranch) -> serde_json::Value {
    match branch {
        LocationBranch::Settlement => serde_json::json!({
            "type": "object",
            "required": ["name", "visual_description", "history_background", "exports", "tone", "authority", "danger_level", "current_tension"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "visual_description": { "type": "string", "minLength": 1 },
                "history_background": { "type": "string", "minLength": 1 },
                "exports": { "type": "array", "minItems": 1, "maxItems": 3, "items": { "type": "string", "minLength": 1 } },
                "tone": { "type": "string", "minLength": 1 },
                "authority": { "type": "string", "minLength": 1 },
                "danger_level": { "type": "string", "enum": LOCATION_DANGER_LEVELS },
                "current_tension": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        // Site + Hideout: exports suppressed, danger GM-locked — both omitted so the
        // model can never emit them.
        LocationBranch::Site | LocationBranch::Hideout => serde_json::json!({
            "type": "object",
            "required": ["name", "visual_description", "history_background", "tone", "authority", "current_tension"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "visual_description": { "type": "string", "minLength": 1 },
                "history_background": { "type": "string", "minLength": 1 },
                "tone": { "type": "string", "minLength": 1 },
                "authority": { "type": "string", "minLength": 1 },
                "current_tension": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        // Guildhall: exports suppressed (omitted), but danger_level is LLM-derived so
        // it stays in the schema. `authority` is overwritten with the faction after.
        LocationBranch::Guildhall => serde_json::json!({
            "type": "object",
            "required": ["name", "visual_description", "history_background", "tone", "authority", "danger_level", "current_tension"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "visual_description": { "type": "string", "minLength": 1 },
                "history_background": { "type": "string", "minLength": 1 },
                "tone": { "type": "string", "minLength": 1 },
                "authority": { "type": "string", "minLength": 1 },
                "danger_level": { "type": "string", "enum": LOCATION_DANGER_LEVELS },
                "current_tension": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
    }
}

/// The user-message seed for the wizard request: a concise restatement of the
/// locked answers. It also doubles as the `@reference` probe, so any place names in
/// the geography resolve against the vault. Reused by the wizard's `build_seed_prompt`
/// to persist the GM's intent as reroll bias.
pub(crate) fn build_wizard_user_prompt(inputs: &LocationWizardInputs) -> String {
    let kind = &inputs.kind_type;
    let mut parts = vec![format!("Create a {kind}.")];
    match location_branch(kind) {
        LocationBranch::Settlement => {
            // A linked faction is emitted as `@factions/<name>` so its vault metadata
            // grounds the prose (mirrors the guildhall); an archetype phrasing carries
            // as plain text. `faction_name` is set only when the GM linked one at the
            // control step (and `control` then equals that name).
            if let Some(faction) = opt_clause(&inputs.faction_name) {
                parts.push(format!("Controlled by @factions/{faction}."));
            } else if let Some(control) = opt_clause(&inputs.control) {
                parts.push(format!("Power structure: {control}."));
            }
            if let Some(resources) = opt_clause(&inputs.resources) {
                parts.push(format!("Natural resources: {resources}."));
            }
            if let Some(mode) = opt_clause(&inputs.export_mode) {
                if mode == "none" {
                    parts.push("Exports: none (a consuming town).".to_string());
                } else {
                    parts.push(format!("Export mode: {mode}."));
                }
            }
        }
        LocationBranch::Site => {
            if let Some(focus) = opt_clause(&inputs.site_focus) {
                parts.push(format!("Focus: {focus}."));
            }
            if let Some(draw) = opt_clause(&inputs.site_draw) {
                parts.push(format!("Draw: {draw}."));
            }
            // A holy-site draw links a deity; `@gods/<name>` pulls its vault metadata
            // into context so the prose stays in that god's domain.
            if let Some(god) = opt_clause(&inputs.site_god) {
                parts.push(format!("Holy to @gods/{god}."));
            }
        }
        LocationBranch::Hideout => {
            // As with the settlement's control, a linked faction owner grounds the prose
            // via `@factions/<name>`; an archetype owner carries as plain text.
            if let Some(faction) = opt_clause(&inputs.faction_name) {
                parts.push(format!("Owner: @factions/{faction}."));
            } else if let Some(owner) = opt_clause(&inputs.base_owner) {
                parts.push(format!("Owner: {owner}."));
            }
            if let Some(protection) = opt_clause(&inputs.base_protection) {
                parts.push(format!("Protection: {protection}."));
            }
            if let Some(purpose) = opt_clause(&inputs.base_purpose) {
                parts.push(format!("Purpose: {purpose}."));
            }
        }
        LocationBranch::Guildhall => {
            // `@factions/<name>` resolves to the faction's authoritative metadata when
            // it is a published note (the reference machinery reads it); a draft or
            // free-typed name simply doesn't resolve and the name carries on its own.
            if let Some(faction) = opt_clause(&inputs.faction_name) {
                parts.push(format!(
                    "The organization that runs this hall: @factions/{faction}."
                ));
            }
            if let Some(role) = opt_clause(&inputs.public_role) {
                parts.push(format!("Public role: {role}."));
            }
            // `@locations/<sub>/<anchor>` pulls the containing place's metadata in the
            // same way — the `@reference` system is path-keyed, so a subfoldered note
            // must carry its subfolder or grounding degrades to name-only. A flat note
            // or free-typed place has no subfolder and falls back to `@locations/<anchor>`.
            if let Some(anchor) = opt_clause(&inputs.location_anchor) {
                match opt_clause(&inputs.location_anchor_sub) {
                    Some(sub) => parts.push(format!("It stands within @locations/{sub}/{anchor}.")),
                    None => parts.push(format!("It stands within @locations/{anchor}.")),
                }
            }
        }
    }
    if let Some(geo) = opt_clause(&inputs.geography) {
        parts.push(format!("Geography: {geo}."));
    }
    if let Some(hint) = opt_clause(&inputs.hint) {
        parts.push(format!("Also: {hint}."));
    }
    parts.join(" ")
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocationSeed {
    pub name: String,
    // GM-locked in the wizard (omitted from the wizard schema), so default to empty
    // when absent; the one-shot schema still requires it. Overwritten from the
    // accumulator in the wizard path.
    #[serde(default)]
    pub kind_type: String,
    #[serde(default)]
    pub kind_custom: Option<String>,
    pub visual_description: String,
    pub history_background: String,
    // Suppressed (Site/Hideout) or derived (Settlement) in the wizard; the
    // site/hideout schema omits it entirely, so tolerate its absence.
    #[serde(default)]
    pub exports: Vec<String>,
    pub tone: String,
    // Suppressed by the one-shot lane (its schema omits it, emptied after); the
    // wizard schemas still require it, so tolerate its absence here.
    #[serde(default)]
    pub authority: String,
    // GM-locked for Site/Hideout (omitted from their schema, injected after).
    #[serde(default)]
    pub danger_level: String,
    pub current_tension: String,
}

fn describe_recent_location_seeds(seeds: &[LocationSeed]) -> String {
    if seeds.is_empty() {
        return "none".to_string();
    }
    seeds
        .iter()
        .take(10)
        .map(|seed| format!("{} | {} | {}", seed.name, seed.kind_type, seed.danger_level))
        .collect::<Vec<_>>()
        .join("; ")
}

fn recent_location_name_set(seeds: &[LocationSeed]) -> std::collections::HashSet<String> {
    seeds
        .iter()
        .map(|seed| seed.name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

impl AiGenerationService {
    pub async fn generate_location_seed(
        &self,
        prompt: Option<String>,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<LocationSeed>, String> {
        let (config, model) = load_generation_config()?;

        let user_prompt = prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Generate one distinct fantasy location for a D&D campaign.");

        let reference_context = build_reference_context(&config, user_prompt).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "location_seed", 20)
            .await?;
        let recent_seeds = parse_recent_seeds::<LocationSeed>(recent_payloads);
        let recent_names = recent_location_name_set(&recent_seeds);
        let recent_context = describe_recent_location_seeds(&recent_seeds);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

        // The one-shot mirrors the ruin/site output shape: prose-first, no exports
        // or settlement-economy modelling. `kind_type` + `danger_level` stay
        // model-derived (no GM to lock them, unlike the wizard's Site branch).
        let schema = serde_json::json!({
            "type": "object",
            "required": ["name", "kind_type", "visual_description", "history_background", "tone", "danger_level", "current_tension"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "kind_type": { "type": "string", "enum": LOCATION_KIND_TYPES },
                "kind_custom": { "type": ["string", "null"] },
                "visual_description": { "type": "string", "minLength": 1 },
                "history_background": { "type": "string", "minLength": 1 },
                "tone": { "type": "string", "minLength": 1 },
                "danger_level": { "type": "string", "enum": LOCATION_DANGER_LEVELS },
                "current_tension": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        });

        let client = OllamaChatClient::from_config(&config)?;
        let reference_suffix = reference_system_suffix(&reference_context);
        let verbosity = config.generation.verbosity;
        let mut seen_attempt_names = HashSet::new();

        let seed = run_seed_attempts(
            &client,
            &model,
            &LOCATION_GEN_SAMPLING,
            config.ollama.num_ctx,
            &schema,
            user_prompt,
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names.",
            "location_seed",
            database,
            generation_repo,
            |note| format!(
                "You generate one usable D&D location seed for a game master — describe the place by its look, its history, and the tension there now, the way you would a ruin, landmark, or remote site (not a modelled settlement economy). Return only JSON with fields name, kind_type, kind_custom, visual_description, history_background, tone, danger_level, current_tension. Pick the kind_type that best fits. tone must be 2-5 words. If kind_type is not other, kind_custom must be null. Do not invent exports, trade goods, rulers, or governments. danger_level must be one of: {danger}. If referenced vault metadata is provided, treat it as authoritative setting context and reuse established canonical names for any region, settlement, or landmark instead of inventing new ones. Avoid these recent seeds: {recent_context}.{note}{reference_suffix}{detail}",
                danger = LOCATION_DANGER_LEVELS.join(", "),
                detail = detail_directive(verbosity),
            ),
            || "failed to generate valid structured location output from ollama".to_string(),
            |seed: LocationSeed| {
                let mut seed = match normalize_location_seed(seed) {
                    Ok(seed) => seed,
                    Err(_) => return SeedStep::Retry,
                };
                // Mirror the ruin/site shape: suppress exports and authority (no
                // economy or rulership modelling) and validate prose only, leaving
                // kind_type + danger_level as the model's choices (normalized above).
                seed.exports = Vec::new();
                seed.authority = String::new();
                if validate_location_prose(&seed).is_err() {
                    return SeedStep::Retry;
                }
                let normalized_name = seed.name.to_ascii_lowercase();
                if recent_names.contains(&normalized_name)
                    || seen_attempt_names.contains(&normalized_name)
                {
                    return SeedStep::Retry;
                }
                seen_attempt_names.insert(normalized_name);
                SeedStep::Accept(seed)
            },
        )
        .await?;

        Ok(SeedGeneration { seed, notice })
    }
    /// The location wizard's kind-aware generation. Mirrors `generate_location_seed`
    /// but (1) the JSON schema is shaped per branch — Settlement keeps
    /// `exports`/`danger_level`, Site/Hideout drop both (GM-locked / suppressed) — and
    /// (2) the GM's locked answers (control, resources, export mode, focus, owner,
    /// protection, purpose, geography) are embedded as authoritative context, so the
    /// model fills the LLM-derived fields *under* them. `kind_type`/`kind_custom` are
    /// never requested; they are overwritten from the accumulator afterward. Only the
    /// freeform custom-kind lane does NOT use this method — it stays on the one-shot
    /// `generate_location_seed`.
    pub async fn generate_location_seed_for_wizard(
        &self,
        inputs: &LocationWizardInputs,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<LocationSeed>, String> {
        let (config, model) = load_generation_config()?;

        let branch = location_branch(&inputs.kind_type);
        let user_prompt = build_wizard_user_prompt(inputs);

        let reference_context = build_reference_context(&config, &user_prompt).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "location_seed", 20)
            .await?;
        let recent_seeds = parse_recent_seeds::<LocationSeed>(recent_payloads);
        let recent_names = recent_location_name_set(&recent_seeds);
        let recent_context = describe_recent_location_seeds(&recent_seeds);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(&user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

        let schema = wizard_location_schema(branch);
        let system_prompt_base = wizard_location_system_prompt(inputs, branch);

        let client = OllamaChatClient::from_config(&config)?;
        let reference_suffix = reference_system_suffix(&reference_context);
        let verbosity = config.generation.verbosity;
        let mut seen_attempt_names = HashSet::new();

        // Locked answers copied out for the (synchronous) accept closure.
        let kind_type = inputs.kind_type.clone();
        let kind_custom = inputs.kind_custom.clone();
        let danger_lock = inputs.danger_lock.clone();
        let faction_name = inputs.faction_name.clone();

        let seed = run_seed_attempts(
            &client,
            &model,
            &LOCATION_GEN_SAMPLING,
            config.ollama.num_ctx,
            &schema,
            &user_prompt,
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names.",
            "location_seed",
            database,
            generation_repo,
            |note| format!(
                "{system_prompt_base} If referenced vault metadata is provided, treat it as authoritative setting context and reuse established canonical names for any region, settlement, or landmark instead of inventing new ones. Avoid reusing these recent location names: {recent_context}.{note}{reference_suffix}{detail}",
                detail = detail_directive(verbosity),
            ),
            || "failed to generate valid structured location output from ollama".to_string(),
            |mut seed: LocationSeed| {
                seed.name = normalize_name(&seed.name);
                if seed.name.is_empty() {
                    return SeedStep::Retry;
                }
                seed.visual_description = normalize_unknown_text(&seed.visual_description);
                seed.history_background = normalize_unknown_text(&seed.history_background);
                seed.tone = normalize_unknown_text(&seed.tone);
                seed.authority = normalize_unknown_text(&seed.authority);
                seed.current_tension = normalize_unknown_text(&seed.current_tension);

                // Kind is GM-locked at step 1 — never the model's pick.
                seed.kind_type = kind_type.clone();
                seed.kind_custom = kind_custom.clone();

                let validation = match branch {
                    LocationBranch::Settlement => {
                        seed.exports = normalize_exports(seed.exports);
                        seed.danger_level = match normalize_location_danger_level(&seed.danger_level)
                        {
                            Ok(value) => value,
                            Err(_) => return SeedStep::Retry,
                        };
                        validate_location_details(&seed)
                    }
                    LocationBranch::Site | LocationBranch::Hideout => {
                        // Exports suppressed; danger is the GM's locked answer.
                        seed.exports = Vec::new();
                        seed.danger_level =
                            danger_lock.clone().unwrap_or_else(|| "Unknown".to_string());
                        validate_location_prose(&seed)
                    }
                    LocationBranch::Guildhall => {
                        // A public HQ: exports suppressed, but danger is LLM-derived
                        // (incidental, like a settlement's) so it must validate.
                        seed.exports = Vec::new();
                        seed.danger_level = match normalize_location_danger_level(&seed.danger_level)
                        {
                            Ok(value) => value,
                            Err(_) => return SeedStep::Retry,
                        };
                        validate_location_prose(&seed)
                    }
                };
                if validation.is_err() {
                    return SeedStep::Retry;
                }

                // A linked faction is the known house; force authority to its name.
                if let Some(name) = &faction_name {
                    seed.authority = name.clone();
                }

                let normalized_name = seed.name.to_ascii_lowercase();
                if recent_names.contains(&normalized_name)
                    || seen_attempt_names.contains(&normalized_name)
                {
                    return SeedStep::Retry;
                }
                seen_attempt_names.insert(normalized_name);
                SeedStep::Accept(seed)
            },
        )
        .await?;

        Ok(SeedGeneration { seed, notice })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_subfolder_maps_each_branch_and_flattens_other() {
        assert_eq!(location_subfolder("hamlet"), Some("settlements"));
        assert_eq!(location_subfolder("town"), Some("settlements"));
        assert_eq!(location_subfolder("city"), Some("settlements"));
        assert_eq!(location_subfolder("ruin"), Some("sites"));
        assert_eq!(location_subfolder("landmark"), Some("sites"));
        assert_eq!(location_subfolder("wilderness"), Some("sites"));
        assert_eq!(location_subfolder("hideout"), Some("hideouts"));
        assert_eq!(location_subfolder("guildhall"), Some("guildhalls"));
        // other / freeform / unknown stay flat (NOT routed into settlements/).
        assert_eq!(location_subfolder("other"), None);
        assert_eq!(location_subfolder(""), None);
        assert_eq!(location_subfolder("village"), None);
    }
    #[test]
    fn location_dir_for_kind_appends_subfolder_or_stays_flat() {
        assert_eq!(
            location_dir_for_kind("locations", "ruin"),
            "locations/sites"
        );
        assert_eq!(
            location_dir_for_kind("locations", "town"),
            "locations/settlements"
        );
        assert_eq!(
            location_dir_for_kind("locations", "guildhall"),
            "locations/guildhalls"
        );
        // other / unknown -> flat (the one-shot lane passes "other" deliberately).
        assert_eq!(location_dir_for_kind("locations", "other"), "locations");
        assert_eq!(location_dir_for_kind("locations", "village"), "locations");
    }
    #[test]
    fn guildhall_prompt_threads_anchor_subfolder_when_present() {
        // A subfoldered anchor must emit the path-keyed `@locations/<sub>/<anchor>`.
        let inputs = LocationWizardInputs {
            kind_type: "guildhall".to_string(),
            location_anchor: Some("Silverhall".to_string()),
            location_anchor_sub: Some("settlements".to_string()),
            ..Default::default()
        };
        let prompt = build_wizard_user_prompt(&inputs);
        assert!(prompt.contains("@locations/settlements/Silverhall"));
    }
    #[test]
    fn guildhall_prompt_falls_back_to_name_only_when_flat() {
        // A flat note / free-typed place has no subfolder -> name-only `@locations/<anchor>`.
        let inputs = LocationWizardInputs {
            kind_type: "guildhall".to_string(),
            location_anchor: Some("Silverhall".to_string()),
            location_anchor_sub: None,
            ..Default::default()
        };
        let prompt = build_wizard_user_prompt(&inputs);
        assert!(prompt.contains("@locations/Silverhall"));
        assert!(!prompt.contains("@locations/settlements"));
    }
    #[test]
    fn settlement_prompt_grounds_a_linked_controlling_faction() {
        // A linked controlling faction is emitted as `@factions/<name>` so its metadata
        // is pulled into context (like the guildhall); a bare archetype stays plain text.
        let linked = LocationWizardInputs {
            kind_type: "town".to_string(),
            control: Some("House Everwood".to_string()),
            faction_name: Some("House Everwood".to_string()),
            ..Default::default()
        };
        let prompt = build_wizard_user_prompt(&linked);
        assert!(prompt.contains("Controlled by @factions/House Everwood."));

        let archetype = LocationWizardInputs {
            kind_type: "town".to_string(),
            control: Some("a noble house or lord".to_string()),
            ..Default::default()
        };
        let prompt = build_wizard_user_prompt(&archetype);
        assert!(prompt.contains("Power structure: a noble house or lord."));
        assert!(!prompt.contains("@factions/"));
    }
    #[test]
    fn settlement_none_export_mode_asks_for_an_empty_exports_list() {
        // A frontier/march town may export nothing — "none" must steer the model to an
        // empty exports list, not coax a 1-3 item list as raw/refined/mixed do.
        let inputs = LocationWizardInputs {
            kind_type: "town".to_string(),
            export_mode: Some("none".to_string()),
            ..Default::default()
        };
        let system = wizard_location_system_prompt(&inputs, LocationBranch::Settlement);
        assert!(system.contains("exports nothing"));
        assert!(system.contains("empty exports list"));
        assert!(!system.contains("Its exports are none"));

        let user = build_wizard_user_prompt(&inputs);
        assert!(user.contains("Exports: none (a consuming town)."));
        assert!(!user.contains("Export mode: none."));
    }
    #[test]
    fn holy_site_grounds_its_linked_god_in_prompt_and_system() {
        // A holy-site draw links a deity: the user prompt emits `@gods/<name>` (pulling
        // its vault note into context) and the system prompt steers the prose into that
        // god's domain — mirroring how the faction wizard grounds its god.
        let inputs = LocationWizardInputs {
            kind_type: "landmark".to_string(),
            site_draw: Some("a holy site devoted to a specific god".to_string()),
            site_god: Some("Maelra".to_string()),
            ..Default::default()
        };
        let user = build_wizard_user_prompt(&inputs);
        assert!(user.contains("Holy to @gods/Maelra."));

        let system = wizard_location_system_prompt(&inputs, LocationBranch::Site);
        assert!(system.contains("holy to Maelra"));

        // A site with no linked god gets neither clause.
        let plain = LocationWizardInputs {
            kind_type: "ruin".to_string(),
            ..Default::default()
        };
        assert!(!build_wizard_user_prompt(&plain).contains("@gods/"));
        assert!(!wizard_location_system_prompt(&plain, LocationBranch::Site).contains("holy to"));
    }
    #[test]
    fn hideout_prompt_grounds_a_linked_owning_faction() {
        let inputs = LocationWizardInputs {
            kind_type: "hideout".to_string(),
            base_owner: Some("The Ashen Veil".to_string()),
            faction_name: Some("The Ashen Veil".to_string()),
            ..Default::default()
        };
        let prompt = build_wizard_user_prompt(&inputs);
        assert!(prompt.contains("Owner: @factions/The Ashen Veil."));
    }
}
