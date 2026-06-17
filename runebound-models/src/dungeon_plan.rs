//! Deterministic content-type assignment for a 5-room dungeon.
//!
//! The model proved unable to pick content types that match their *meaning*
//! (it tags a sewer-crawl `forge`, a passive climax `foreshadowing`), so we take
//! the choice away from it: each beat's type is rolled here from a per-beat
//! weighted table and handed to the LLM as a constraint, not requested as output.
//!
//! Two taxonomies, kept apart on purpose (feature-dungeons.md §2, §7):
//! - **Anchor types** are *rooms* that can BE a beat: combat, cache, forge,
//!   puzzle, offshoot, sidekick, oddity. Each beat gets exactly one, from
//!   [`BEAT_TABLES`].
//! - **Overlays** (foreshadowing, history, map) *layer onto* a beat and can never
//!   be one on their own — so they live in a separate, sparse roll, never in the
//!   anchor table. This makes "Climax = foreshadowing" impossible by construction.
//! - **Factions** is a dungeon-wide *tint*, not a tile — a separate boolean that,
//!   when set, reskins the combat climax into a standoff.
//!
//! Dungeon-level appearance rates are a *union* over five beats, so a type
//! sprinkled thinly across many beats becomes common; to stay rare (forge ~10%)
//! a type must live in essentially one signature beat. The weights below are
//! tuned to: combat ~93%, puzzle ~82%, cache ~77%, oddity ~42%, offshoot ~50%,
//! sidekick ~20% (entrance-only), forge ~12% (see the statistical test below).

/// A small, dependency-free deterministic PRNG (SplitMix64). Seedable so the roll
/// is reproducible in tests; call sites seed it from wall-clock micros, matching
/// how the ollama retry seeds are derived.
pub struct PlanRng(u64);

impl PlanRng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, n)`. `n` must be > 0.
    fn below(&mut self, n: u32) -> u32 {
        (self.next_u64() % u64::from(n)) as u32
    }
}

type Weight = (&'static str, u32);

// Per-beat anchor weights (each column sums to 100). Order matches
// DUNGEON_FUNCTIONS: Entrance, Puzzle, Setback, Climax, Resolution.
// Sidekick is ENTRANCE-ONLY: a weak model reliably introduces the companion in
// the opening beat regardless of where the roll places it, so we stop fighting it
// and only allow a sidekick at beat 1, where "meet an ally who then accompanies
// you" is exactly right. ~20% of dungeons get one; the rest never do.
const ENTRANCE: &[Weight] = &[("combat", 28), ("puzzle", 30), ("offshoot", 22), ("sidekick", 20)];
const PUZZLE: &[Weight] = &[("combat", 24), ("puzzle", 54), ("offshoot", 16), ("forge", 6)];
const SETBACK: &[Weight] = &[("combat", 36), ("puzzle", 28), ("offshoot", 22), ("oddity", 14)];
const CLIMAX: &[Weight] = &[("combat", 60), ("puzzle", 16), ("oddity", 24)];
const RESOLUTION: &[Weight] = &[
    ("cache", 76),
    ("oddity", 8),
    ("offshoot", 12),
    ("forge", 4),
];

/// The five per-beat anchor tables, indexed by beat (0 = Entrance … 4 = Resolution).
pub const BEAT_TABLES: [&[Weight]; 5] = [ENTRANCE, PUZZLE, SETBACK, CLIMAX, RESOLUTION];

// Overlay layer — lean by default (feature-dungeons.md §8). ~45% of dungeons get
// exactly one overlay; the rest get none. Two-overlay dungeons are GM opt-in only.
const OVERLAY_CHANCE_PCT: u32 = 45;
const OVERLAYS: &[Weight] = &[("foreshadowing", 40), ("history", 40), ("map", 20)];

// Dungeon-wide faction tint.
const FACTION_CHANCE_PCT: u32 = 25;

/// An overlay placed on a specific beat (it layers onto that beat's anchor type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedOverlay {
    pub beat_index: usize,
    pub overlay_type: String,
}

/// The resolved content assignment for one dungeon: one anchor type per beat,
/// an optional overlay, and the dungeon-wide faction tint flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DungeonContentPlan {
    pub anchors: [String; 5],
    pub overlay: Option<PlannedOverlay>,
    pub factions: bool,
}

/// Pick one entry from a weighted table, optionally excluding a single type
/// (used to forbid adjacent duplicates). `table` must retain positive total
/// weight after the exclusion — true for every beat here, since each beat has
/// at least three eligible types and we exclude at most one.
fn weighted_pick(rng: &mut PlanRng, table: &[Weight], exclude: Option<&str>) -> String {
    let total: u32 = table
        .iter()
        .filter(|(t, _)| Some(*t) != exclude)
        .map(|(_, w)| *w)
        .sum();
    let mut roll = rng.below(total);
    for &(t, w) in table {
        if Some(t) == exclude {
            continue;
        }
        if roll < w {
            return t.to_string();
        }
        roll -= w;
    }
    // Numerically unreachable; fall back to the last eligible entry.
    table
        .iter()
        .rev()
        .find(|(t, _)| Some(*t) != exclude)
        .map(|(t, _)| t.to_string())
        .unwrap_or_default()
}

/// Beats an overlay can sensibly attach to, by fit (feature-dungeons.md §7):
/// foreshadowing hooks at the threshold or the payoff; history is lore early; a
/// map reveals best from a side passage, else the payoff.
fn overlay_fit_beats(overlay: &str, anchors: &[String; 5]) -> Vec<usize> {
    match overlay {
        "foreshadowing" => vec![0, 4],
        "history" => vec![0, 1],
        "map" => {
            let offshoots: Vec<usize> = anchors
                .iter()
                .enumerate()
                .filter(|(_, t)| t.as_str() == "offshoot")
                .map(|(i, _)| i)
                .collect();
            if offshoots.is_empty() {
                vec![4]
            } else {
                offshoots
            }
        }
        _ => vec![4],
    }
}

/// Roll a full content plan from `seed`. Anchors honor the no-adjacent-duplicate
/// rule (except combat, where a Setback fight feeding a Climax fight is a wanted
/// ambush→boss escalation).
pub fn roll_dungeon_content_plan(seed: u64) -> DungeonContentPlan {
    let mut rng = PlanRng::new(seed);

    let mut anchors: Vec<String> = Vec::with_capacity(5);
    for (i, table) in BEAT_TABLES.iter().enumerate() {
        // Forbid repeating the previous beat's type, unless it was combat.
        let prev = if i > 0 { Some(anchors[i - 1].clone()) } else { None };
        let exclude = prev.as_deref().filter(|p| *p != "combat");
        anchors.push(weighted_pick(&mut rng, table, exclude));
    }
    let anchors: [String; 5] = anchors
        .try_into()
        .expect("BEAT_TABLES has exactly five beats");

    let overlay = if rng.below(100) < OVERLAY_CHANCE_PCT {
        let overlay_type = weighted_pick(&mut rng, OVERLAYS, None);
        let fit = overlay_fit_beats(&overlay_type, &anchors);
        let beat_index = fit[rng.below(fit.len() as u32) as usize];
        Some(PlannedOverlay {
            beat_index,
            overlay_type,
        })
    } else {
        None
    };

    let factions = rng.below(100) < FACTION_CHANCE_PCT;

    DungeonContentPlan {
        anchors,
        overlay,
        factions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_tables_each_sum_to_one_hundred() {
        for (i, table) in BEAT_TABLES.iter().enumerate() {
            let total: u32 = table.iter().map(|(_, w)| *w).sum();
            assert_eq!(total, 100, "beat {i} weights must sum to 100");
        }
    }

    #[test]
    fn sidekick_appears_only_at_the_entrance() {
        // The model can't reliably hold a sidekick to a later beat, so it is
        // allowed ONLY at the entrance (beat 0) and nowhere else.
        let weight = |table: &[Weight], t: &str| {
            table.iter().find(|(n, _)| *n == t).map(|(_, w)| *w).unwrap_or(0)
        };
        let per_beat: Vec<u32> = BEAT_TABLES.iter().map(|t| weight(t, "sidekick")).collect();
        assert!(per_beat[0] > 0, "sidekick should be available at the entrance");
        for (i, w) in per_beat.iter().enumerate().skip(1) {
            assert_eq!(*w, 0, "sidekick must not appear at beat {i}: {per_beat:?}");
        }
    }

    #[test]
    fn same_seed_is_deterministic() {
        let a = roll_dungeon_content_plan(42);
        let b = roll_dungeon_content_plan(42);
        assert_eq!(a, b);
    }

    #[test]
    fn anchors_only_ever_use_eligible_types_per_beat() {
        for seed in 0..2_000u64 {
            let plan = roll_dungeon_content_plan(seed);
            for (i, anchor) in plan.anchors.iter().enumerate() {
                let eligible: Vec<&str> = BEAT_TABLES[i].iter().map(|(t, _)| *t).collect();
                assert!(
                    eligible.contains(&anchor.as_str()),
                    "beat {i} rolled ineligible type {anchor}"
                );
            }
        }
    }

    #[test]
    fn no_adjacent_duplicates_except_combat() {
        for seed in 0..5_000u64 {
            let plan = roll_dungeon_content_plan(seed);
            for pair in plan.anchors.windows(2) {
                if pair[0] == pair[1] {
                    assert_eq!(
                        pair[0], "combat",
                        "only combat may repeat across adjacent beats (seed {seed})"
                    );
                }
            }
        }
    }

    #[test]
    fn overlays_never_land_in_the_anchor_set() {
        // Structural guarantee: an overlay type can never be an anchor.
        for seed in 0..2_000u64 {
            let plan = roll_dungeon_content_plan(seed);
            for anchor in &plan.anchors {
                assert!(
                    !OVERLAYS.iter().any(|(t, _)| t == anchor),
                    "overlay type leaked into anchors: {anchor}"
                );
            }
            if let Some(overlay) = &plan.overlay {
                assert!(overlay.beat_index < 5);
            }
        }
    }

    #[test]
    fn appearance_rates_match_design_targets() {
        // Statistical check over many seeds; tolerances are wide enough to absorb
        // sampling noise but tight enough to catch a mis-tuned table.
        let n = 40_000u64;
        let mut combat = 0;
        let mut puzzle = 0;
        let mut cache = 0;
        let mut oddity = 0;
        let mut forge = 0;
        let mut overlay = 0;
        let mut factions = 0;
        for seed in 0..n {
            let plan = roll_dungeon_content_plan(seed);
            let has = |t: &str| plan.anchors.iter().any(|a| a == t);
            combat += has("combat") as u64;
            puzzle += has("puzzle") as u64;
            cache += has("cache") as u64;
            oddity += has("oddity") as u64;
            forge += has("forge") as u64;
            overlay += plan.overlay.is_some() as u64;
            factions += plan.factions as u64;
        }
        let pct = |c: u64| (c as f64 / n as f64) * 100.0;
        let within = |actual: f64, target: f64, tol: f64| (actual - target).abs() <= tol;

        eprintln!(
            "RATES combat={:.1} puzzle={:.1} cache={:.1} oddity={:.1} forge={:.1} overlay={:.1} factions={:.1}",
            pct(combat), pct(puzzle), pct(cache), pct(oddity), pct(forge), pct(overlay), pct(factions)
        );
        // Targets are the emergent rates these weights actually produce (the
        // adjacency rule renormalizes excluded weight onto the heaviest type, so
        // combat lands a touch above its naive ~90%). Weights are the design
        // artifact; these assertions guard against accidental retuning.
        assert!(within(pct(combat), 93.0, 3.0), "combat {:.1}%", pct(combat));
        assert!(within(pct(puzzle), 82.0, 4.0), "puzzle {:.1}%", pct(puzzle));
        assert!(within(pct(cache), 77.0, 4.0), "cache {:.1}%", pct(cache));
        assert!(within(pct(oddity), 42.0, 4.0), "oddity {:.1}%", pct(oddity));
        assert!(within(pct(forge), 12.0, 3.0), "forge {:.1}%", pct(forge));
        assert!(within(pct(overlay), 45.0, 4.0), "overlay {:.1}%", pct(overlay));
        assert!(within(pct(factions), 25.0, 4.0), "factions {:.1}%", pct(factions));
    }
}
