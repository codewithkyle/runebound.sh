use super::AiGenerationService;
use super::engine::*;
use super::reference::*;

use crate::repositories::{Database, GenerationRepository};
use crate::services::ollama_chat::{OllamaChatClient, detail_directive, load_generation_config};
use crate::utils::{estimate_tokens, normalize_faction_seed, validate_faction_details};
use runebound_models::utils::FACTION_KIND_TYPES;
use std::collections::HashSet;

/// The three faction categories each of the 9 kinds rolls up into (design §3). Drives
/// the wizard branch (D5), the persisted `category` column/frontmatter (D2), and the
/// under-`factions/` subfolder. The exact analogue of the `location_*` chain below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactionCategory {
    Houses,
    Establishments,
    Religion,
}

/// Map a kind to its category, or `None` for an unknown kind. The faction scheme has
/// no `other`, so `None` means "not one of the 9" (a drifted row), the way
/// [`location_subfolder`] treats freeform locations.
pub fn faction_category(kind_type: &str) -> Option<FactionCategory> {
    match kind_type {
        "great_house" | "major_vassal" | "minor_vassal" | "individual_lord" => {
            Some(FactionCategory::Houses)
        }
        "guild" | "company" | "criminal_syndicate" => Some(FactionCategory::Establishments),
        "temple" | "cult" => Some(FactionCategory::Religion),
        _ => None,
    }
}

/// The persisted category string for a kind: `"houses" | "establishments" |
/// "religion"`, or `""` for an unknown kind (kept lenient so a drifted row still
/// saves; D2). Used to fill the frontmatter/row `category`.
pub fn faction_category_str(kind_type: &str) -> &'static str {
    match faction_category(kind_type) {
        Some(FactionCategory::Houses) => "houses",
        Some(FactionCategory::Establishments) => "establishments",
        Some(FactionCategory::Religion) => "religion",
        None => "",
    }
}

/// Under-`factions/` subfolder for a kind, or `None` for an unknown kind (stays
/// flat). The category folder doubles as the subfolder, so this is just
/// [`faction_category_str`] with `""` collapsed to `None` — mirroring
/// [`location_subfolder`]. Only shapes the readable `.md` vault_path; the TOML store
/// and DB projection stay on the flat `factions/` base.
pub fn faction_subfolder(kind_type: &str) -> Option<&'static str> {
    match faction_category_str(kind_type) {
        "" => None,
        sub => Some(sub),
    }
}

/// Full relative dir for a NEW faction save: `factions/<sub>` for a known kind, or
/// flat `factions` otherwise. Mirrors [`location_dir_for_kind`].
pub fn faction_dir_for_kind(base: &str, kind_type: &str) -> String {
    match faction_subfolder(kind_type) {
        Some(sub) => format!("{base}/{sub}"),
        None => base.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Faction wizard generation (§5.1) — the lord-type / control-type / mandate /
// reach / brand vocab (wizard-prompt only, never persisted), the locked-answer
// inputs, the per-category prompt + schema, and the wizard generation method.
// Mirrors the `LocationWizard*` chain above.
// ---------------------------------------------------------------------------

/// Houses power bases (design §4). Each does triple duty: sets wealth/sphere,
/// auto-seeds the WOAC Obstacle via its built-in vulnerability (Appendix B), and
/// pre-shapes the seat-location built later. Canonical tokens; the wizard owns the
/// display labels.
pub const LORD_TYPES: [&str; 6] = [
    "chokepoint",
    "surplus",
    "junction",
    "specialist",
    "march",
    "extraction",
];

/// Establishments control types (design §8.2) — the lord-type analog for a guild /
/// company / syndicate; each carries its own Obstacle vulnerability.
pub const CONTROL_TYPES: [&str; 5] = ["craft", "service", "trade", "vice", "knowledge"];

/// Religion mandates (design §8.3) — what the god demands; the power-base analog for
/// a temple / cult, each with its own Obstacle vulnerability.
pub const MANDATES: [&str; 6] = [
    "devotion",
    "sacrifice",
    "conquest",
    "purity",
    "secret_knowledge",
    "cycle",
];

/// How far an establishment or faith reaches; scales `sphere_of_influence`.
pub const REACH: [&str; 3] = ["local", "regional", "realm"];

/// What a Great House is known for (design §8.1, Q3a); a menu the wizard also lets
/// the GM override with custom free text.
pub const HOUSE_BRANDS: [&str; 6] = [
    "wealth", "loyalty", "martial", "piety", "cunning", "lineage",
];

/// `(lever, vulnerability → obstacle)` for a houses lord-type (design §4, Appendix B).
/// `None` for an unknown token. The vulnerability is fed to the LLM so the generated
/// `obstacle` is grounded in the power base's built-in fault line.
fn lord_type_facts(token: &str) -> Option<(&'static str, &'static str)> {
    Some(match token {
        "chokepoint" => (
            "controlling a terrain bottleneck (a pass, strait, or forest road) and taking tolls on forced passage",
            "an alternate route opens and the tolls collapse",
        ),
        "surplus" => (
            "aggregating and storing the region's production surplus in granaries, warehouses, and distribution",
            "spoilage, a raid, or a market glut",
        ),
        "junction" => (
            "owning a transport-mode interchange (road-to-river, river-to-coast) and charging fees on every transfer",
            "a rival port or route draws the traffic away",
        ),
        "specialist" => (
            "refining raw goods into denser, higher-value forms before shipping (grain to spirits, wool to cloth)",
            "an input supply is cut, or the technique is copied",
        ),
        "march" => (
            "defending the realm's edge in exchange for delegated military autonomy and land",
            "peace lets the crown reclaim the autonomy, while war makes them the first to fall",
        ),
        "extraction" => (
            "holding a point-source of a scarce necessity (ore, salt, stone) as a monopoly",
            "the vein runs dry or floods, or a richer deposit opens elsewhere",
        ),
        _ => return None,
    })
}

/// `(what it does, vulnerability → obstacle)` for an establishment control type
/// (design §8.2, Appendix B). `None` for an unknown token.
fn control_type_facts(token: &str) -> Option<(&'static str, &'static str)> {
    Some(match token {
        "craft" => (
            "producing a crafted good (smithing, alchemy, masonry)",
            "a rival guild or a cheap substitute undercuts the monopoly",
        ),
        "service" => (
            "selling a service or force (mercenaries, assassins, spies)",
            "it is only as good as its last job — a defeat or a betrayal",
        ),
        "trade" => (
            "moving goods (caravans, shipping, brokerage)",
            "a rival route, a new tariff, or a revoked charter",
        ),
        "vice" => (
            "running contraband or vice (smuggling, gambling, narcotics, theft)",
            "the law, a rival crew, or a crackdown",
        ),
        "knowledge" => (
            "trading in knowledge and influence (spymasters, fixers, money-lenders)",
            "a leaked secret, or a debt called in",
        ),
        _ => return None,
    })
}

/// `(what the god demands, vulnerability → obstacle)` for a religion mandate
/// (design §8.3, Appendix B). `None` for an unknown token.
fn mandate_facts(token: &str) -> Option<(&'static str, &'static str)> {
    Some(match token {
        "devotion" => (
            "devotion and tribute — worship and offerings",
            "donor fatigue, or a richer rival temple",
        ),
        "sacrifice" => (
            "sacrifice — blood, lives, or valuables",
            "the supply of victims runs out, or public backlash",
        ),
        "conquest" => (
            "conquest and conversion — spreading the faith",
            "resistance, or a crusade against them",
        ),
        "purity" => (
            "purity and law — enforcing a moral or ritual order",
            "a schism over who is pure, or purges",
        ),
        "secret_knowledge" => (
            "secret knowledge — forbidden lore",
            "the secret leaks, or rival seekers close in",
        ),
        "cycle" => (
            "the cycle of nature — death and rebirth, the seasons, the wilds",
            "a broken cycle, or encroaching civilization",
        ),
        _ => return None,
    })
}

/// The built-in fault line for a vassal/lord loyalty type (design §6, Appendix B),
/// fed alongside the liege so it surfaces in the Obstacle. `None` for an unknown token.
fn loyalty_fault(token: &str) -> Option<&'static str> {
    Some(match token {
        "reward" => "rewards slight everyone passed over and inflate the next demand",
        "marriage" => "a marriage bond splits loyalty and turns kin into hostages",
        "military" => {
            "one failure cracks it, and a vassal grown too strong is feared rather than trusted"
        }
        "economic" => "a rival's better terms can buy the bond away",
        "shared_enemy" => "the bond dissolves the moment the common enemy is gone",
        "oath" => "an oath lasts only while someone still cares that it was sworn",
        "secret" => "a shared secret makes enemies the day it is used as blackmail",
        _ => return None,
    })
}

/// A human phrase for a reach token, used to scale `sphere_of_influence`.
fn reach_phrase(token: &str) -> &'static str {
    match token {
        "local" => "a single locale (one town, valley, or quarter)",
        "regional" => "a region (several settlements or a province)",
        "realm" => "the whole realm",
        _ => "an unspecified area",
    }
}

/// The GM's locked faction-wizard answers, flattened into a borrow-friendly struct
/// the wizard fills and passes to generation. The relational fields that feed the
/// LLM as *grounding* (leader, liege, loyalty, patron, god) live here; the ones that
/// are only ever linked/blank (allies, rivals) do not. Grounding is not generation:
/// the leader is still picker-set, excluded from the schema, and never rerolled (D3)
/// — feeding its name/metadata just keeps the generated prose consistent with the
/// established leader instead of inventing a different one. Mirrors
/// [`LocationWizardInputs`].
#[derive(Debug, Clone, Default)]
pub struct FactionWizardInputs {
    pub kind_type: String,
    // Houses
    pub power_base: Option<String>,
    pub power_specifics: Option<String>,
    pub brand: Option<String>,
    pub liege: Option<String>,
    pub loyalty_type: Option<String>,
    // Establishments
    pub control_type: Option<String>,
    pub control_specifics: Option<String>,
    // Establishments + Religion
    pub reach: Option<String>,
    pub patron: Option<String>,
    // Religion
    pub god: Option<String>,
    pub mandate: Option<String>,
    pub mandate_specifics: Option<String>,
    // Shared tail. `leader` is the linked NPC (grounding only, never generated).
    pub leader: Option<String>,
    pub want: Option<String>,
    pub hint: Option<String>,
}

const FACTION_WOAC_LEASH: &str = " public_description must be 1-3 sentences; reputation 1-2 sentences; symbol_description exactly 1 sentence; want, obstacle, action, and consequence each 1-2 sentences.";

/// Build the branch-specific system prompt that embeds the GM's locked answers and
/// bakes in the design's generation rules — the WOAC engine, the visible/hidden gap,
/// the Obstacle pre-seeded from the chosen power base's vulnerability (Appendix B),
/// and the per-category framing. The recent-seed avoidance, repair note, reference
/// block, and detail directive are appended by the caller's closure.
fn wizard_faction_system_prompt(inputs: &FactionWizardInputs, category: FactionCategory) -> String {
    let kind = &inputs.kind_type;
    let mut prompt = format!(
        "You generate one usable D&D faction seed (a {kind}) for a game master, built on the WOAC engine. Return only JSON with fields name, public_description, reputation, symbol_description, want, obstacle, action, consequence, sphere_of_influence, resources_assets. The visible face — public_description (its public claim to legitimacy), reputation (how others regard it), symbol_description (its sigil, colors, or banner, exactly one sentence) — is what the faction shows the world; the real leverage shows through the engine. want = the faction's deep aim. obstacle = what stands in its way. action = what it is doing about it. consequence = what lands on the table if it wins or loses. Do not invent a leader, allies, rivals, a liege, or a headquarters — the game master adds those.{leash}",
        leash = FACTION_WOAC_LEASH,
    );

    match category {
        FactionCategory::Houses => {
            if let Some((lever, vuln)) = opt_clause(&inputs.power_base).and_then(lord_type_facts) {
                prompt.push_str(&format!(
                    " This house's power comes from {lever}; resources_assets and sphere_of_influence must reflect that. Seed the obstacle from its built-in vulnerability: {vuln}."
                ));
            }
            if let Some(spec) = opt_clause(&inputs.power_specifics) {
                prompt.push_str(&format!(" Specifically, its holding is: {spec}."));
            }
            if kind == "great_house" {
                if let Some(brand) = opt_clause(&inputs.brand) {
                    prompt.push_str(&format!(
                        " It is known above all for {brand}; weight its public_description and reputation toward that."
                    ));
                }
                prompt.push_str(
                    " As a Great House it sits at the apex and answers to no one; it cannot directly assault its peers — it moves through proxies, vassals, and leverage. Scale sphere_of_influence to a realm-spanning house.",
                );
            } else if kind == "individual_lord" && opt_clause(&inputs.liege).is_none() {
                // A free-agent individual lord built its own holding and is sworn to no
                // house, so it must not be handed a liege. (A lord the GM *did* link to an
                // overlord has `liege` set and falls through to the sworn branch below.)
                prompt.push_str(
                    " It is a self-made individual lord and a free agent — sworn to no house and answering to no liege or overlord; do not invent one. Scale sphere_of_influence to a single independent holding, smaller than a Great House.",
                );
            } else {
                let liege = opt_clause(&inputs.liege).unwrap_or("its liege");
                prompt.push_str(&format!(" It is sworn to {liege}."));
                if let Some(loyalty) = opt_clause(&inputs.loyalty_type)
                    && let Some(fault) = loyalty_fault(loyalty)
                {
                    prompt.push_str(&format!(
                        " The bond is one of {loyalty}; let that loyalty's fault line surface in the obstacle: {fault}."
                    ));
                }
                prompt.push_str(
                    " Scale sphere_of_influence to its layer — a vassal or lord answers upward and holds less than a Great House.",
                );
            }
        }
        FactionCategory::Establishments => {
            if let Some((what, vuln)) =
                opt_clause(&inputs.control_type).and_then(control_type_facts)
            {
                prompt.push_str(&format!(
                    " It makes its living by {what}; resources_assets must reflect that. Seed the obstacle from that vulnerability: {vuln}."
                ));
            }
            if let Some(spec) = opt_clause(&inputs.control_specifics) {
                prompt.push_str(&format!(" Specifically: {spec}."));
            }
            if let Some(reach) = opt_clause(&inputs.reach) {
                prompt.push_str(&format!(
                    " Its reach is {}; scale sphere_of_influence to that.",
                    reach_phrase(reach)
                ));
            }
            if let Some(patron) = opt_clause(&inputs.patron) {
                prompt.push_str(&format!(
                    " It operates under the charter or protection of {patron}; that dependency is itself a fault line."
                ));
            }
            if kind == "criminal_syndicate" {
                prompt.push_str(
                    " As a criminal syndicate, widen the gap between its public front and its true racket.",
                );
            } else {
                prompt.push_str(
                    " As a guild or company, keep its public face and its real business close.",
                );
            }
        }
        FactionCategory::Religion => {
            if let Some(god) = opt_clause(&inputs.god) {
                prompt.push_str(&format!(
                    " It serves {god}; keep its creed and methods consistent with that deity's domain."
                ));
            }
            if let Some((demands, vuln)) = opt_clause(&inputs.mandate).and_then(mandate_facts) {
                prompt.push_str(&format!(
                    " The god demands {demands}; seed the obstacle from that mandate's vulnerability: {vuln}."
                ));
            }
            if let Some(spec) = opt_clause(&inputs.mandate_specifics) {
                prompt.push_str(&format!(" Specifically: {spec}."));
            }
            if let Some(reach) = opt_clause(&inputs.reach) {
                prompt.push_str(&format!(
                    " Its reach is {}; scale sphere_of_influence to that.",
                    reach_phrase(reach)
                ));
            }
            if let Some(patron) = opt_clause(&inputs.patron) {
                prompt.push_str(&format!(" It is backed by {patron}."));
            }
            if kind == "cult" {
                prompt.push_str(
                    " As a cult, widen the gap between its public creed and its true creed — its public_description hides what the want and action reveal — and sharpen the obstacle toward exposure and suppression.",
                );
            } else {
                prompt.push_str(
                    " As a temple, keep its public faith and its true creed aligned; sharpen the obstacle toward schism and rival faiths.",
                );
            }
        }
    }

    if let Some(leader) = opt_clause(&inputs.leader) {
        prompt.push_str(&format!(
            " The game master has already named this faction's leader: {leader}. Wherever the prose refers to its leadership, use that exact name and keep it consistent with any referenced metadata about them; do not invent a different leader."
        ));
    }

    if let Some(want) = opt_clause(&inputs.want) {
        prompt.push_str(&format!(
            " The game master has fixed the faction's Want: {want}. Build the obstacle, action, and consequence to serve that Want."
        ));
    }

    prompt
}

/// The wizard's WOAC schema: every LLM-filled field, **omitting** `kind_type`
/// (GM-locked, re-applied after generation) and every relational field (never
/// generated, D3). One schema serves all three categories — only the *prompt*
/// differs by branch. Mirrors [`wizard_location_schema`].
fn wizard_faction_schema(_category: FactionCategory) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["name", "public_description", "reputation", "symbol_description", "want", "obstacle", "action", "consequence", "sphere_of_influence", "resources_assets"],
        "properties": {
            "name": { "type": "string", "minLength": 1 },
            "public_description": { "type": "string", "minLength": 1 },
            "reputation": { "type": "string", "minLength": 1 },
            "symbol_description": { "type": "string", "minLength": 1 },
            "want": { "type": "string", "minLength": 1 },
            "obstacle": { "type": "string", "minLength": 1 },
            "action": { "type": "string", "minLength": 1 },
            "consequence": { "type": "string", "minLength": 1 },
            "sphere_of_influence": { "type": "string", "minLength": 1 },
            "resources_assets": { "type": "array", "minItems": 1, "maxItems": 5, "items": { "type": "string", "minLength": 1 } }
        },
        "additionalProperties": false
    })
}

/// The user-message seed for the wizard request: a concise restatement of the
/// locked answers that doubles as the `@reference` probe — it emits `@npcs/<leader>`,
/// `@gods/<god>`, `@factions/<liege>`, and `@factions/<patron>` tokens so each linked
/// entity's metadata is pulled into context (mirroring the guildhall's
/// `@factions/<name>`). Reused by the wizard's `build_seed_prompt` to persist GM
/// intent as reroll bias.
pub(crate) fn build_faction_wizard_user_prompt(inputs: &FactionWizardInputs) -> String {
    let kind = &inputs.kind_type;
    let mut parts = vec![format!("Create a {kind}.")];
    match faction_category(kind) {
        Some(FactionCategory::Houses) => {
            if let Some(power) = opt_clause(&inputs.power_base) {
                parts.push(format!("Power base: {power}."));
            }
            if let Some(spec) = opt_clause(&inputs.power_specifics) {
                parts.push(format!("Holding: {spec}."));
            }
            if let Some(brand) = opt_clause(&inputs.brand) {
                parts.push(format!("Known for: {brand}."));
            }
            if let Some(liege) = opt_clause(&inputs.liege) {
                parts.push(format!("Sworn to @factions/{liege}."));
            }
            if let Some(loyalty) = opt_clause(&inputs.loyalty_type) {
                parts.push(format!("Loyalty type: {loyalty}."));
            }
        }
        Some(FactionCategory::Establishments) => {
            if let Some(control) = opt_clause(&inputs.control_type) {
                parts.push(format!("Controls: {control}."));
            }
            if let Some(spec) = opt_clause(&inputs.control_specifics) {
                parts.push(format!("Specifics: {spec}."));
            }
            if let Some(reach) = opt_clause(&inputs.reach) {
                parts.push(format!("Reach: {reach}."));
            }
            if let Some(patron) = opt_clause(&inputs.patron) {
                parts.push(format!("Chartered or protected by @factions/{patron}."));
            }
        }
        Some(FactionCategory::Religion) => {
            if let Some(god) = opt_clause(&inputs.god) {
                parts.push(format!("Serves @gods/{god}."));
            }
            if let Some(mandate) = opt_clause(&inputs.mandate) {
                parts.push(format!("Mandate: {mandate}."));
            }
            if let Some(spec) = opt_clause(&inputs.mandate_specifics) {
                parts.push(format!("Specifics: {spec}."));
            }
            if let Some(reach) = opt_clause(&inputs.reach) {
                parts.push(format!("Reach: {reach}."));
            }
            if let Some(patron) = opt_clause(&inputs.patron) {
                parts.push(format!("Backed by @factions/{patron}."));
            }
        }
        None => {}
    }
    if let Some(leader) = opt_clause(&inputs.leader) {
        parts.push(format!("Led by @npcs/{leader}."));
    }
    if let Some(want) = opt_clause(&inputs.want) {
        parts.push(format!("Ambition (Want): {want}."));
    }
    if let Some(hint) = opt_clause(&inputs.hint) {
        parts.push(format!("Also: {hint}."));
    }
    parts.join(" ")
}

/// The LLM-filled fields of a faction (design §5 WOAC engine). The relational/place
/// fields — `leader`, `allies`, `rivals_enemies`, `liege`, `loyalty_type`,
/// `headquarters` — are intentionally absent: they are picker-linked or left blank,
/// never generated (D3).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FactionSeed {
    pub name: String,
    // GM-locked in the wizard (omitted from the wizard schema), so default to empty
    // when absent; the one-shot schema still requires it and the model picks one of
    // the 9 kinds. Overwritten from the accumulator in the wizard path. Mirrors
    // `LocationSeed.kind_type`.
    #[serde(default)]
    pub kind_type: String,
    // Visible face.
    pub public_description: String,
    pub reputation: String,
    pub symbol_description: String,
    // WOAC engine.
    pub want: String,
    pub obstacle: String,
    pub action: String,
    pub consequence: String,
    pub sphere_of_influence: String,
    pub resources_assets: Vec<String>,
}

fn recent_faction_name_set(seeds: &[FactionSeed]) -> std::collections::HashSet<String> {
    seeds
        .iter()
        .map(|seed| seed.name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

fn describe_recent_faction_seeds(seeds: &[FactionSeed]) -> String {
    if seeds.is_empty() {
        return "none".to_string();
    }
    seeds
        .iter()
        .take(10)
        .map(|seed| format!("{} | {} | {}", seed.name, seed.kind_type, seed.reputation))
        .collect::<Vec<_>>()
        .join("; ")
}

impl AiGenerationService {
    pub async fn generate_faction_seed(
        &self,
        prompt: Option<String>,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<FactionSeed>, String> {
        let (config, model) = load_generation_config()?;

        let user_prompt = prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Generate one distinct fantasy faction for a D&D campaign.");

        let reference_context = build_reference_context(&config, user_prompt).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "faction_seed", 20)
            .await?;
        let recent_seeds = parse_recent_seeds::<FactionSeed>(recent_payloads);
        let recent_names = recent_faction_name_set(&recent_seeds);
        let recent_context = describe_recent_faction_seeds(&recent_seeds);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);
        let enforce_unique_name = reference_context.system_context.is_empty();

        // WOAC schema (design §5): the visible face + the want/obstacle/action/
        // consequence spine. No relational/place fields (leader/allies/rivals/liege/
        // loyalty/headquarters) — the GM adds those (D3). `kind_type` is one of the 9.
        let schema = serde_json::json!({
            "type": "object",
            "required": ["name", "kind_type", "public_description", "reputation", "symbol_description", "want", "obstacle", "action", "consequence", "sphere_of_influence", "resources_assets"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "kind_type": { "type": "string", "enum": FACTION_KIND_TYPES },
                "public_description": { "type": "string", "minLength": 1 },
                "reputation": { "type": "string", "minLength": 1 },
                "symbol_description": { "type": "string", "minLength": 1 },
                "want": { "type": "string", "minLength": 1 },
                "obstacle": { "type": "string", "minLength": 1 },
                "action": { "type": "string", "minLength": 1 },
                "consequence": { "type": "string", "minLength": 1 },
                "sphere_of_influence": { "type": "string", "minLength": 1 },
                "resources_assets": { "type": "array", "minItems": 1, "maxItems": 5, "items": { "type": "string", "minLength": 1 } }
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
            &FACTION_GEN_SAMPLING,
            config.ollama.num_ctx,
            &schema,
            user_prompt,
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names.",
            "faction_seed",
            database,
            generation_repo,
            |note| format!(
                "You generate usable D&D faction seeds built on the WOAC engine. Return only JSON with fields name, kind_type, public_description, reputation, symbol_description, want, obstacle, action, consequence, sphere_of_influence, resources_assets. kind_type must be one of the 9 kinds: {kinds}. Pick the one that best fits. The visible face — public_description, reputation, symbol_description — is the faction's public claim; the real leverage and friction show through the WOAC fields. want = the faction's deep aim (1-2 sentences). obstacle = what stands in its way (1-2 sentences). action = what it is doing about it (1-2 sentences). consequence = what is at stake if it wins or loses (1-2 sentences). symbol_description should be exactly 1 sentence describing symbol/sigil/colors/banner/iconography. Do not invent a leader, allies, rivals, a liege, or a headquarters — the game master adds those. If referenced vault metadata includes an established name for an organization, group, guild, or house, reuse that exact canonical name instead of inventing a new one. Avoid these recent seeds: {recent}.{note}{reference_suffix}{detail}",
                kinds = FACTION_KIND_TYPES.join(", "),
                recent = recent_context,
                reference_suffix = reference_suffix,
                detail = detail_directive(verbosity),
            ),
            || "failed to generate valid structured faction output from ollama".to_string(),
            |seed: FactionSeed| {
                let seed = match normalize_faction_seed(seed) {
                    Ok(seed) => seed,
                    Err(_) => return SeedStep::Retry,
                };
                if validate_faction_details(&seed).is_err() {
                    return SeedStep::Retry;
                }
                let normalized_name = seed.name.to_ascii_lowercase();
                if enforce_unique_name
                    && (recent_names.contains(&normalized_name)
                        || seen_attempt_names.contains(&normalized_name))
                {
                    return SeedStep::Retry;
                }
                if enforce_unique_name {
                    seen_attempt_names.insert(normalized_name);
                }
                SeedStep::Accept(seed)
            },
        )
        .await?;

        Ok(SeedGeneration { seed, notice })
    }
    /// The wizard path: generate the WOAC fields *under* the GM's locked answers
    /// (category/kind, power base, liege/loyalty, control type, mandate, reach,
    /// patron, god), mirroring `generate_location_seed_for_wizard`. The schema omits
    /// `kind_type` (GM-locked, re-applied in the accept closure) and every relational
    /// field (never generated, D3). When the GM seeded a Want, it is locked verbatim
    /// after validation; otherwise the model infers it from the locked answers.
    pub async fn generate_faction_seed_for_wizard(
        &self,
        inputs: &FactionWizardInputs,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<FactionSeed>, String> {
        let (config, model) = load_generation_config()?;

        // An unknown kind can't reach the wizard (it routes by category), but fall
        // back defensively to Houses so generation still produces something usable.
        let category = faction_category(&inputs.kind_type).unwrap_or(FactionCategory::Houses);
        let user_prompt = build_faction_wizard_user_prompt(inputs);

        let reference_context = build_reference_context(&config, &user_prompt).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "faction_seed", 20)
            .await?;
        let recent_seeds = parse_recent_seeds::<FactionSeed>(recent_payloads);
        let recent_names = recent_faction_name_set(&recent_seeds);
        let recent_context = describe_recent_faction_seeds(&recent_seeds);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(&user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);
        let enforce_unique_name = reference_context.system_context.is_empty();

        let schema = wizard_faction_schema(category);
        let system_prompt_base = wizard_faction_system_prompt(inputs, category);

        let client = OllamaChatClient::from_config(&config)?;
        let reference_suffix = reference_system_suffix(&reference_context);
        let verbosity = config.generation.verbosity;
        let mut seen_attempt_names = HashSet::new();

        // Locked answers copied out for the (synchronous) accept closure.
        let kind_type = inputs.kind_type.clone();
        let want_lock = opt_clause(&inputs.want).map(str::to_string);

        let seed = run_seed_attempts(
            &client,
            &model,
            &FACTION_GEN_SAMPLING,
            config.ollama.num_ctx,
            &schema,
            &user_prompt,
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names.",
            "faction_seed",
            database,
            generation_repo,
            |note| format!(
                "{system_prompt_base} If referenced vault metadata is provided, treat it as authoritative setting context and reuse established canonical names for any organization, house, deity, or place instead of inventing new ones. Avoid reusing these recent faction names: {recent_context}.{note}{reference_suffix}{detail}",
                detail = detail_directive(verbosity),
            ),
            || "failed to generate valid structured faction output from ollama".to_string(),
            |seed: FactionSeed| {
                // Kind is GM-locked at the category/kind steps — never the model's pick.
                let candidate = FactionSeed {
                    kind_type: kind_type.clone(),
                    ..seed
                };
                let mut seed = match normalize_faction_seed(candidate) {
                    Ok(seed) => seed,
                    Err(_) => return SeedStep::Retry,
                };
                if validate_faction_details(&seed).is_err() {
                    return SeedStep::Retry;
                }
                // A GM-seeded Want is locked verbatim (it survives validation above on
                // the model's inference, then replaces it); skipped → the model's stands.
                if let Some(want) = &want_lock {
                    seed.want = want.clone();
                }
                let normalized_name = seed.name.to_ascii_lowercase();
                if enforce_unique_name
                    && (recent_names.contains(&normalized_name)
                        || seen_attempt_names.contains(&normalized_name))
                {
                    return SeedStep::Retry;
                }
                if enforce_unique_name {
                    seen_attempt_names.insert(normalized_name);
                }
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

    // ----------------------------------------------------------------------
    // Faction wizard generation (Phase 5 §5.1)
    // ----------------------------------------------------------------------
    #[test]
    fn faction_subfolder_maps_all_nine_kinds() {
        // Houses.
        for kind in [
            "great_house",
            "major_vassal",
            "minor_vassal",
            "individual_lord",
        ] {
            assert_eq!(faction_subfolder(kind), Some("houses"), "{kind}");
        }
        // Establishments.
        for kind in ["guild", "company", "criminal_syndicate"] {
            assert_eq!(faction_subfolder(kind), Some("establishments"), "{kind}");
        }
        // Religion.
        for kind in ["temple", "cult"] {
            assert_eq!(faction_subfolder(kind), Some("religion"), "{kind}");
        }
        // An unknown kind stays flat (no `other` in the faction scheme; D1).
        assert_eq!(faction_subfolder("other"), None);
        assert_eq!(faction_subfolder(""), None);
    }
    #[test]
    fn faction_dir_for_kind_appends_category_or_stays_flat() {
        assert_eq!(
            faction_dir_for_kind("factions", "major_vassal"),
            "factions/houses"
        );
        assert_eq!(
            faction_dir_for_kind("factions", "guild"),
            "factions/establishments"
        );
        assert_eq!(
            faction_dir_for_kind("factions", "cult"),
            "factions/religion"
        );
        // A one-shot / unknown kind publishes flat.
        assert_eq!(faction_dir_for_kind("factions", ""), "factions");
    }
    #[test]
    fn faction_wizard_prompt_emits_reference_tokens_for_houses() {
        // Houses vassal: the liege is probed as `@factions/<name>` so its metadata is
        // pulled into context.
        let inputs = FactionWizardInputs {
            kind_type: "major_vassal".to_string(),
            power_base: Some("extraction".to_string()),
            liege: Some("House Vaurel".to_string()),
            loyalty_type: Some("oath".to_string()),
            want: Some("Corner the salt trade".to_string()),
            ..Default::default()
        };
        let prompt = build_faction_wizard_user_prompt(&inputs);
        assert!(prompt.contains("Create a major_vassal."));
        assert!(prompt.contains("Power base: extraction."));
        assert!(prompt.contains("@factions/House Vaurel"));
        assert!(prompt.contains("Loyalty type: oath."));
        assert!(prompt.contains("Ambition (Want): Corner the salt trade."));
    }
    #[test]
    fn faction_wizard_grounds_the_linked_leader_in_prompt_and_system() {
        // A picked leader NPC is fed as `@npcs/<name>` so its vault metadata is pulled
        // into context, and the system prompt names it so the prose stays consistent
        // instead of inventing a different leader — while it never enters the schema (D3).
        let inputs = FactionWizardInputs {
            kind_type: "great_house".to_string(),
            power_base: Some("march".to_string()),
            leader: Some("Lord Everwood".to_string()),
            ..Default::default()
        };
        let user_prompt = build_faction_wizard_user_prompt(&inputs);
        assert!(user_prompt.contains("Led by @npcs/Lord Everwood."));

        let system_prompt = wizard_faction_system_prompt(&inputs, FactionCategory::Houses);
        assert!(system_prompt.contains("already named this faction's leader: Lord Everwood"));
        assert!(system_prompt.contains("do not invent a different leader"));
    }
    #[test]
    fn faction_wizard_prompt_emits_god_and_patron_tokens_for_religion() {
        // Religion: the god is `@gods/<name>` and the patron is `@factions/<name>`.
        let inputs = FactionWizardInputs {
            kind_type: "cult".to_string(),
            god: Some("Maelra".to_string()),
            mandate: Some("sacrifice".to_string()),
            patron: Some("House Vaurel".to_string()),
            ..Default::default()
        };
        let prompt = build_faction_wizard_user_prompt(&inputs);
        assert!(prompt.contains("@gods/Maelra"));
        assert!(prompt.contains("Mandate: sacrifice."));
        assert!(prompt.contains("@factions/House Vaurel"));
    }
    #[test]
    fn faction_wizard_prompt_emits_patron_token_for_establishments() {
        let inputs = FactionWizardInputs {
            kind_type: "guild".to_string(),
            control_type: Some("craft".to_string()),
            reach: Some("regional".to_string()),
            patron: Some("House Vaurel".to_string()),
            ..Default::default()
        };
        let prompt = build_faction_wizard_user_prompt(&inputs);
        assert!(prompt.contains("Controls: craft."));
        assert!(prompt.contains("Reach: regional."));
        assert!(prompt.contains("@factions/House Vaurel"));
    }
    #[test]
    fn wizard_faction_schema_omits_kind_type_and_relational_fields() {
        // The schema is GM-locked-kind + WOAC only: `kind_type` and every relational
        // field (leader/allies/rivals/liege/loyalty) are excluded (D3).
        for category in [
            FactionCategory::Houses,
            FactionCategory::Establishments,
            FactionCategory::Religion,
        ] {
            let schema = wizard_faction_schema(category);
            let props = schema["properties"].as_object().expect("properties object");
            for required in [
                "name",
                "public_description",
                "reputation",
                "symbol_description",
                "want",
                "obstacle",
                "action",
                "consequence",
                "sphere_of_influence",
                "resources_assets",
            ] {
                assert!(
                    props.contains_key(required),
                    "schema must declare {required}"
                );
            }
            for forbidden in [
                "kind_type",
                "category",
                "leader",
                "allies",
                "rivals_enemies",
                "liege",
                "loyalty_type",
                "headquarters",
            ] {
                assert!(
                    !props.contains_key(forbidden),
                    "schema must omit {forbidden}"
                );
            }
            // Closed schema: the model can't slip extra fields in.
            assert_eq!(
                schema["additionalProperties"],
                serde_json::Value::Bool(false)
            );
        }
    }
    #[test]
    fn wizard_faction_system_prompt_seeds_obstacle_from_lord_type_vulnerability() {
        // Each houses lord-type pre-seeds the Obstacle with its built-in fault line
        // (Appendix B): March → the crown reclaims the autonomy; Extraction → the vein.
        let march = FactionWizardInputs {
            kind_type: "great_house".to_string(),
            power_base: Some("march".to_string()),
            ..Default::default()
        };
        let prompt = wizard_faction_system_prompt(&march, FactionCategory::Houses);
        assert!(prompt.contains("Seed the obstacle from its built-in vulnerability"));
        assert!(prompt.contains("crown reclaim the autonomy"));
        // Great House framing: no direct peer assault.
        assert!(prompt.contains("cannot directly assault its peers"));

        let extraction = FactionWizardInputs {
            kind_type: "minor_vassal".to_string(),
            power_base: Some("extraction".to_string()),
            liege: Some("House Vaurel".to_string()),
            loyalty_type: Some("oath".to_string()),
            ..Default::default()
        };
        let prompt = wizard_faction_system_prompt(&extraction, FactionCategory::Houses);
        assert!(prompt.contains("the vein runs dry"));
        // Vassal/lord framing: the liege + the loyalty fault line both surface.
        assert!(prompt.contains("sworn to House Vaurel"));
        assert!(prompt.contains("oath"));
        assert!(prompt.contains("only while someone still cares"));
    }
    #[test]
    fn wizard_faction_system_prompt_seeds_obstacle_from_control_type_and_mandate() {
        // Establishment control type → its vulnerability.
        let vice = FactionWizardInputs {
            kind_type: "criminal_syndicate".to_string(),
            control_type: Some("vice".to_string()),
            reach: Some("local".to_string()),
            ..Default::default()
        };
        let prompt = wizard_faction_system_prompt(&vice, FactionCategory::Establishments);
        assert!(prompt.contains("the law, a rival crew, or a crackdown"));
        // Syndicate widens the public/true gap.
        assert!(prompt.contains("widen the gap between its public front and its true racket"));

        // Religion mandate → its vulnerability, sharpened by the cult tone.
        let cult = FactionWizardInputs {
            kind_type: "cult".to_string(),
            god: Some("Maelra".to_string()),
            mandate: Some("secret_knowledge".to_string()),
            reach: Some("regional".to_string()),
            ..Default::default()
        };
        let prompt = wizard_faction_system_prompt(&cult, FactionCategory::Religion);
        assert!(prompt.contains("the secret leaks"));
        assert!(prompt.contains("widen the gap between its public creed and its true creed"));
    }
    #[test]
    fn wizard_faction_system_prompt_locks_gm_want() {
        let inputs = FactionWizardInputs {
            kind_type: "great_house".to_string(),
            power_base: Some("chokepoint".to_string()),
            brand: Some("ancient lineage".to_string()),
            want: Some("Hold the only bridge over the Ironwash".to_string()),
            ..Default::default()
        };
        let prompt = wizard_faction_system_prompt(&inputs, FactionCategory::Houses);
        // A GM-seeded Want is named in the prompt and the rest of WOAC is told to serve it.
        assert!(prompt.contains("fixed the faction's Want"));
        assert!(prompt.contains("Hold the only bridge over the Ironwash"));
        // The Great House brand colors the visible face.
        assert!(prompt.contains("known above all for ancient lineage"));
    }
}
