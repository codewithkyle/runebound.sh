# Feature: Dungeons (5-Room Dungeon Oracle)

> **Purpose:** Capture the design intent and decisions for the dungeon generation
> feature *before* implementation. This is a concept doc, not a playbook — it
> records what the feature is, the mental model behind it, the decisions we've
> locked, and the questions still open.

---

## 1. What This Is

A **generator that emits a 5-beat dungeon skeleton** a GM can publish and then
tweak into their own world.

It is an **oracle, not an author.** The goal is *clear, constrained, structured*
output that gives just enough of a creative kickstart for the GM to weave and
tweak the narrative — not a finished module to run cold. The GM owns the story;
the tool removes the blank-page friction.

The enemy of an oracle is **over-generation.** A model left loose will write
paragraphs of boxed text per room, which is the failure mode here: harder to
tweak, it smothers the GM's own imagination, and it buries the structure under
prose. The north star for every generated field is **specific but unresolved** —
concrete enough to throw a spark, but it never tells the GM the answer. The
oracle plants questions, not conclusions.

Creation runs as a dedicated **multi-step flow** (like the setup flow), not a
single command — see §3 — and the output is headed by a one-line statement of the
dungeon's spine (§6).

---

## 2. Core Mental Model: Beats vs. Contents

Two distinct taxonomies, on orthogonal axes. Keeping them separate is the whole
game.

- **The 5 rooms are *beats*** — fixed dramatic *functions* in a sequence. The
  skeleton never changes; there are always five.
- **The 11 room types are *contents*** — a *palette* of things that can fill a
  beat.

Generation is therefore: **assign contents to a fixed beat skeleton, paint with
theme, tune with detail dials.** The combinatorics
(`beats × content-type × theme × dials`) are where "limitless possibilities"
actually come from — a handful of axes multiply into effectively infinite output.

The "nine forms" from the source article are read as **topologies** (spatial flow
shapes over the five beats), not a separate concept — see §6.

---

## 3. The Creation Flow

Dungeon creation **breaks from the single-command pattern.** This is the most
advanced feature in the tool, so it earns a dedicated **multi-step creation flow**
(modeled on the existing setup flow) instead of cramming all context into one
command call. The flow walks the GM through five questions, then generates:

- **A — Premise.** Enter a custom premise, or have the LLM generate one. This is
  the optional seed: with a premise, generation is biased toward it; without one,
  the LLM invents a small, self-contained dungeon to fill space. The premise is a
  *bias*, not a required field — the same feature serves both "I just need filler"
  and "this has to set up X."
- **B — Tone: tragedy or comedy.** The overall emotional polarity of the dungeon.
- **C — Twist: false victory, false defeat, or neither.** The shape of the middle
  beats — do players think they've won and then lose it (false victory), think
  they've lost and then recover (false defeat), or is it played straight (neither)?
- **D — Context.** A free-form chance to seed the LLM with references, established
  world details, constraints, or links to other content the dungeon should set up.
- **E — Topology.** Pick one of the nine forms (§6.1), or none.

Tone dials (B, C) are therefore **GM-facing knobs**, set up front — the GM steers
the mood; the oracle does not surprise them on tone. (They still double as variety
levers across rerolls.)

**Linkage placement.** When the premise (A) or context (D) points the dungeon at
external content, **concentrate that linkage** in the beats built to reach outside
— the **Resolution** and the connective content types (**Map**, **Foreshadowing**,
**Oddity**). Don't smear it across all five beats; keep the body self-contained and
runnable even while it points somewhere.

---

## 4. The Five Beats (the Skeleton)

Fixed order, fixed dramatic function. Maps to classic story structure
(setup → turn → rising tension → climax → payoff).

1. **The Entrance** — setup. What stopped a random NPC from entering? Hidden,
   gated by a check, guarded?
2. **The Puzzle** — the opposite of the Entrance: roleplay, puzzle, or trap.
3. **The Setback** — the meat. Halts forward progress: a twist, a trap, a one-way
   exit that forces them back. The purpose the dungeon exists for.
4. **The Climax** — the boss battle. Tactical, surprising, possibly a monologue
   or a duel.
5. **The Resolution** — the payoff: reward, plot twist, or humble pie.

"5 rooms" is a structure for storytelling within a space — **not** literally five
physical rooms.

---

## 5. The Output Unit: the Beat Card

Every beat renders as the same tight, index-card shape (never a manuscript). If a
beat needs a paragraph, the oracle is doing the GM's job for them.

| Field | What it is | Notes |
|---|---|---|
| **Function** | Fixed label (Entrance / Puzzle / Setback / Climax / Resolution) | Never changes. The spine the GM hangs edits on. |
| **Content type** | Which of the 11, as a tag (`[Combat]`, `[Faction]`…) | Gives the GM a handle; quietly teaches the vocabulary. |
| **Idea** | One or two lines of *what happens here* | The premise expressed through this beat. For combat, this carries *tactics/behavior*, not creatures. |
| **Lever** | One complication, question, or hook the GM can pull | The oracle magic — the bit that makes the GM lean forward. |
| **Loot** | *Conditional* reward line | See §5.1. Present only where the beat earns it. |
| **Read-aloud** | 1–2 sentence static visual description | See §5.2. Doubles as a map-making seed. |

### 5.1 Loot is conditional, not uniform

Do **not** hang a loot line on all five beats. Reward concentrates at the
**payoff** — the Resolution, sometimes the Climax (the boss's hoard), and any
Cache offshoot. The **Setback** is where players *pay*, not collect. Absence is
meaningful: `Loot: none — this is the beat where they bleed` communicates pacing
better than a forced trinket. Weighted to the payoff, blank at the cost. The GM
is expected to swap specific items after publish.

> **No monster suggestions.** Seasoned GMs have ample 3rd-party manuals and will
> slot the right creature themselves. A creature line would also couple output to
> one ruleset and date it. Combat beats convey *how the fight feels* (tactics,
> behavior) via the Idea field instead.

### 5.2 Read-aloud is static, visual, and tight

- **Static and visual only** — shape, scale, materials, light, one or two notable
  objects. No action, no NPC behavior; action read-aloud goes stale the instant
  players do something unexpected, while a static scene travels and is exactly
  what a map-maker needs to start drawing.
- **Tightest leash of any field** (most likely to balloon): what the eye lands on
  first, then one detail implying the beat's function.
- **Dual purpose:** table read-aloud *and* a cartography seed for GMs who draw
  their own maps.

### 5.3 Design principle: one object, triple duty

The most evocative output is when the read-aloud's notable object is *also* the
loot's hiding place *and* the lever's hook. "A cold anvil the size of a cart" is
the image, the thing to map, the dead forge to reignite, and where the item is
buried — at once. This overlap is "specific but unresolved" paying off, and it
keeps the fields from reading like independent dice rolls.

---

## 6. Dungeon-Level Fields & Output Header

Sit above the five cards.

- **Premise / spine (top line)** — a single-line summary of the dungeon's spine
  sits at the **very top of the output**, above all five beats. Whether GM-typed
  (flow step A) or LLM-invented, it is the highest-leverage tweak surface: edit
  that one line, respin, and all five beats re-aim. Showing the through-line is
  exactly what kick-starts the GM's creative juices as they go to refine.
- **Topology** — chosen explicitly in the flow (step E): one of the nine forms,
  or none. *(This supersedes the earlier "auto-select by default" idea — it is now
  an interactive step.)* Topology is *qualitative* (a flow shape), not
  *quantitative* (a count), which is why it hands the cartographer a shape to start
  from without ever committing to a room count.
  - Selection/biasing can follow the premise (a desperate escape wants a
    middle-entrance/looping form; an investigation wants a hub).
  - The chosen form should **talk to the Setback** especially — that's the
    "forces them back / one-way exit" beat, so a middle-entrance form literally
    *is* a Setback that dumps players back toward the Entrance.
- **No room count.** Deliberately omitted: the LLM won't produce an accurate
  number, and exposing a count invites "generate a 50-room dungeon." Topology
  conveys spatial shape without a number.

### 6.1 The Nine Forms (topologies)

> The nine names below are **confirmed against the source diagrams** (read off the
> article by the GM). The structure also holds from first principles: the article's
> *construction rule* forces exactly nine topologies, and all nine names land on
> them cleanly (3 + 4 + 2), with no leftovers or collisions.

The article builds the nine from one construction rule:

> Start with the entrance room. Pick any existing room, add a hallway to a new
> room. Repeat until there are five rooms.

This always yields a **tree** of five rooms (no loops). Abstracting away *where*
the entrance sits, there are exactly **three** five-room tree shapes:

- **The Railroad** — a straight line: `●—●—●—●—●`
- **The Arrow** — a line with one branch (a fork): one room has three exits
- **The Cross** — a central hub with four rooms off it (a star)

Restoring the entrance (which room you start in) splits those three into **nine**,
since each shape has a different number of distinct entry points:

| Base shape | Distinct entrances | Forms |
|---|---|---|
| Railroad (line) | end / one-in / middle | 3 |
| Arrow (fork) | hub / hub-leaf / bend / tail-end | 4 |
| Cross (hub) | hub / spoke | 2 |

3 + 4 + 2 = **9**. (Drop the construction rule entirely and the count rises to 21
— the article has a sequel, *The Twenty-one Forms*.)

The article's nine named forms, grouped by base shape. Each base shape lends its
name to the family member entered at its most natural point (line→**Railroad**,
fork→**Arrow**, star→**Cross**):

**Railroad family** — a straight line; 3 forms by entry point:

1. **The Railroad** — five rooms in sequence; enter one end, payoff at the far end.
   `[E]—●—●—●—●`
2. **The Moose** — the entrance branches into a short arm of 1 room and a long arm
   of 3 sequential rooms. *(A line, entered one-in.)* `●—[E]—●—●—●`
3. **The V for Vendetta** — the entrance branches into two arms of 2 sequential
   rooms, running opposite directions. *(A line, entered at the middle.)*
   `●—●—[E]—●—●`

**Arrow family** — a fork; one room is a 3-exit junction; 4 forms by entry point:

4. **The Arrow** — enter the junction itself: it forks three ways — two single
   rooms and one 2-room corridor.
5. **The Fauchard Fork** — the entrance leads to a room that forks: one path of 1
   room, one path of 2 sequential rooms. *(Entered at a hub-leaf, beside the
   junction.)*
6. **The Evil Mule** — the entrance branches: one path is 1 room; the other leads
   to a room that forks again into 1 room each. *(Entered at the bend.)*
7. **Foglio's Snail** — the entrance leads through 2 sequential rooms, then splits
   into 2 separate rooms — handy for hidden side rooms. *(Entered at the tail-end.)*

**Cross family** — a 4-exit hub; 2 forms by entry point:

8. **The Paw** — the entrance leads to a room that forks into 3 single rooms.
   *(A hub with the entrance hanging off one spoke.)*
9. **The Cross** — the entrance opens directly onto 4 single rooms. *(Entered at
   the hub.)*

---

## 7. The 11 Content Types (the Palette)

They are **not a flat list of the same kind of thing** — they layer onto beats
differently:

- **Encounter spaces** (literal rooms): **combat**, **cache** (loot/rewards),
  **forge** (make magic items), **puzzle**, **offshoot** (optional passage / dead
  end).
- **An NPC:** **sidekick** — a dungeon-specific ally who joins for the dungeon and
  leaves after.
- **Information overlays** (attach to any room): **foreshadowing** (of the dungeon
  or the wider campaign), **history** (lore of the place/people/monsters),
  **map** (reveals hexcrawl/world context — connective tissue between dungeons).
- **A world-object:** **oddity** — the reason the dungeon exists; significant to
  the shape of the world. A campaign-scale artifact that happens to sit in a room.
- **A dungeon-wide dynamic:** **factions** — exist within/around the dungeon; can
  like/hate the party and each other. A global tint, not a single tile.

### The puzzle is a locked door with the flavor changed

A dungeon puzzle is **a locked door, reskinned.** Avoid Simon Says / rhythm /
logic puzzles (they stop play and can't be reliably generated). The mechanic:
chain **locked-door → key** steps. Example: the door needs the forge on → the
forge won't run because of a pipe leak → the pipes are guarded by monsters. Each
step is a gate with a key.

**Conceptual underpinning:** a locked-door→key chain is a dependency graph (DAG),
which *is* generatable and guaranteed solvable. The whole dungeon is the same
shape at a larger scale — the Climax is the final gate, the Resolution sits behind
it, the earlier beats are the keys. Puzzle and dungeon are one structure at two
scales.

---

## 8. Design Principles (Output Behavior)

- **Specific but unresolved** — the north star (§1).
- **Lean by default, richness opt-in** — default output is the lean five beats.
  Extras (faction layer, sidekick, foreshadowing hook on a specific room,
  topology overrides) are things the GM *asks for*, up front or as tweaks. Don't
  fire all 11 types + every modifier into one dungeon.
- **Per-beat reroll** — the GM will love four cards and hate one. Respinning a
  single beat must keep the other four frozen *and* respect them (a new Setback
  still follows the Entrance and feeds the Climax). Partial regeneration against
  frozen context is a core oracle behavior, not a whole-dungeon re-roll.
- **Range across rerolls** — repeated spins can't converge to goblins-trap-boss.
  Variety comes from *which content type fills which beat* (and the tone dials from
  the flow, §3), not just repainted flavor text.
- **Show the spine** — the visible premise line (§6) keeps the whole thing
  steerable.

---

## 9. Composition & Campaign Connective Tissue *(future-facing)*

The structure is **fractal.** 5-room dungeons stack and link: one dungeon's
Resolution or Map becomes the next's Entrance/hook. The "return-to" content types
(**foreshadowing**, **history**, **oddity**) are the threads that weave dungeons
into a campaign; the **oddity** especially is a world-level artifact. A campaign
is itself a 5-beat arc whose beats are dungeons.

v1 generates a single 5-room dungeon, but the beat skeleton is designed to
compose upward — keep that door open.

---

## 10. Decisions Log & Remaining Item

**Resolved (from the design conversation):**

1. **Tone dials → GM-facing, set in the flow.** Tragedy/comedy (step B) and false
   victory / false defeat / neither (step C) are explicit up-front questions in the
   creation flow (§3). The GM steers the mood; the dials also keep rerolls varied.
2. **Topologies → pulled from the article.** The nine are structurally pinned down
   (§6.1) from the article's construction rule and presented to the GM in flow
   step E (pick one, or none).
3. **Premise spine line → ships.** A single-line premise/spine summary sits at the
   very top of the output (§6).

4. **Topology names → confirmed and mapped.** All nine forms are named and pinned
   to their layouts in §6.1 (Railroad, Moose, V for Vendetta, Arrow, Fauchard Fork,
   Evil Mule, Foglio's Snail, Paw, Cross), each falling out of the 3 base shapes ×
   entrance placement (3 + 4 + 2 = 9).

**Remaining:** none open.

---

## 11. Source

- *The Nine Forms of the Five Room Dungeon* —
  https://gnomestew.com/the-nine-forms-of-the-five-room-dungeon/

---

*Last updated: 2026-06-16*
