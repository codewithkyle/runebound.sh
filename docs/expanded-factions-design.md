# Expanded Factions Design (v0.7.0)

> **Status:** design complete — classification, schema, pickers, and all three
> per-branch question flows (§8) are locked. Ready for an implementation spec.

## 1. Goal

Make factions a first-class step in the worldbuilding loop:

**create NPC → create faction (with the NPC as leader) → create location (controlled by the faction)**

These stay **three separate, independent commands** (`create npc`, `create
faction`, `create location`). We are **not** encoding a single guided pipeline —
we are *enabling* the workflow by letting each command **optionally link** to
entities the others already created (the faction wizard's leader picker reads
from existing NPCs; the guildhall's authority reads from existing factions; and
so on). The order above is just the natural one, because each picker references
things that already exist — nothing enforces it, and every link is optional with
a fallback (skip → free-text, or a blank section to fill in later).

The first and third legs already exist (NPC one-shot; the Location wizard's
guildhall branch links a location's authority to a faction). v0.7.0 adds the
missing middle — a **faction creation wizard** that mirrors the location wizard:
the quick **one-shot lane is retained**, and a structured, grounded **wizard** is
added on top.

## 2. Core principle: factions are power centers, not flavor labels

The wizard operationalizes the campaign's worldbuilding model:

- **The political question:** who controls what, and why does everyone else
  accept it? Every faction has a *visible* layer (legitimacy, reputation, the
  public face) and a *hidden* layer (the force or leverage behind it).
- **WOAC engine:** every faction runs on **Want → Obstacle → Action →
  Consequence**. This is the generative spine of the entity (see §5).
- **Feudalism is a supply chain with a crown:** lords exist because someone
  solved a real logistics problem. This drives the houses power-base menu (§4).
- **Three political layers:** Great Houses → Vassals → Individual Lords, encoded
  directly as the houses kinds.

## 3. Classification

Three categories drive the wizard branch, the question set, and the output
subfolder. `kind` plays a different role in each category.

| Category | Subfolder | Kinds | Role of `kind` |
|---|---|---|---|
| **houses** | `factions/houses/` | Great House · Major Vassal · Minor Vassal · Individual Lord | the **political layer** (scale + constraints) |
| **establishments** | `factions/establishments/` | Guild · Company · Criminal Syndicate | **flavor** (smiths / mercenaries / thieves) |
| **religion** | `factions/religion/` | Temple · Cult | **tone** — legit vs shadowy = the visible/hidden gap |
| *(one-shot / freeform)* | `factions/` (flat) | — | unclassified quick-create |

Notes:

- Picking Temple vs Cult (or Guild/Company vs Syndicate) **is** the
  good-vs-shadowy / legit-vs-illicit answer — the kind carries the tone, so no
  separate question is needed.
- **Temple** chosen over "Church" — setting-neutral, pairs with Cult, fits
  polytheism.
- This **replaces** the old 10-kind enum. Orphaned archetypes fold in as flavor
  with no dedicated kind: holy military order → Temple or Company; rebellion /
  revolutionary cell → Cult or Syndicate; mercenaries → Company; mages → Guild;
  bandits → Syndicate.
- There is **no `other` wizard kind** — the one-shot lane
  (`create faction <prompt>`) covers freeform and writes flat to `factions/`,
  exactly like the location one-shot.
- Subfoldering is gated by a transient `wizard_subfoldered` flag (mirrors the
  location wizard).

## 4. Houses power base: the lord-types

The heart of the houses branch. A house's power comes from solving a logistics
problem; the type chosen does triple duty — it sets the faction's wealth &
sphere, **auto-seeds the WOAC Obstacle** (each type's built-in vulnerability),
and **pre-shapes the seat-location** you build for them later.

| Lord-type | Solves | Lever | Vulnerability → Obstacle | Seeds location |
|---|---|---|---|---|
| **Chokepoint** | terrain bottleneck (pass/strait/forest road) | tolls on forced passage | an alternate route kills it | toll/border fort |
| **Surplus** | aggregating & storing production surplus | granaries, warehouses, distribution | spoilage, raid, glut | granary town |
| **Junction** | transport-mode interchange (road↔river↔coast) | fees on every transfer; owns neutral ground | a rival port/route | market port |
| **Specialist** | refining goods for density+value before shipping | value-add monopoly (grain→spirits, wool→cloth) | input supply cut, technique copied | mill/craft town |
| **March** | defending the realm's edge | delegated military autonomy + land | peace lets the crown reclaim it; war makes you first to fall; independence invites rebellion | frontier keep |
| **Extraction** | point-source high-value resource (ore/salt/stone) | monopoly on a scarce necessity (salt = preservation) | the vein runs dry or floods; a richer deposit opens elsewhere | mining town |

Three more — **Mill** (banal monopoly), **Headwaters** (irrigation), **Charter**
(market franchise) — were considered and held as optional setting flavor, not in
the core menu.

## 5. Schema: WOAC replaces the loose fields

WOAC is promoted to **real fields** and **replaces** the fuzzy motivation fields:

| WOAC field | Absorbs | Notes |
|---|---|---|
| **Want** | `true_agenda` (+ `goals_short_term` / `goals_long_term` as near/far laddering) | the deep aim |
| **Obstacle** | `current_tension` | auto-seeded by the lord-type's vulnerability for houses |
| **Action** | `methods` | how they pursue the Want |
| **Consequence** | *(new)* | the hook that lands on the table |

Resulting faction shape:

- **Identity:** id, name, slug, kind, category, vault_path, timestamps
- **Visible face:** public_description, reputation, symbol_description
- **Engine (WOAC):** want, obstacle, action, consequence
- **Structure & web:** leader (NPC link), sphere_of_influence, resources_assets,
  allies, rivals_enemies
- **Houses-only (Vassal/Lord), persisted & rendered:** liege (faction link), loyalty_type
- **Rendered-only (not stored):** headquarters (see §7)

Removed from the struct: `true_agenda`, `methods`, `current_tension`,
`goals_short_term`, `goals_long_term` (absorbed by WOAC) and `headquarters`.

## 6. Entity links (pickers)

The wizard links to entities the user already created — aligned with the
create-order (NPC → faction → location):

| Picker | Branches | Reuse | Required |
|---|---|---|---|
| **Leader** (NPC) | all | new machinery | skip → free-text |
| **God** | religion | new machinery | per-branch (deferred) |
| **Liege** (Great House faction) | houses: Major/Minor Vassal, Individual Lord | reuse faction picker | yes for vassals/lords |
| **Allies** (factions) | all | reuse faction picker | **skippable** (see §7) |
| **Rivals** (factions) | all | reuse faction picker | **skippable** (see §7) |

After a vassal/lord picks their **liege**, ask the **loyalty type** (enum; option
`0 = random` — reward / marriage / military / economic / shared-enemy / oath /
secret). The liege + loyalty type are fed to the LLM as grounding; each loyalty
type carries its own built-in fault line.

## 7. Allies, rivals, and HQ: link or leave blank — never auto-generate

To avoid an ever-growing web of invented faction names the user has to clean up,
the relational and place fields are **never generated by the LLM**:

- **Allies** and **Rivals** are two **separate, skippable picker steps**. Link an
  existing faction → it renders as a wikilink. Skip → the published note carries a
  **blank section** to fill in later in Obsidian.
- **HQ (headquarters)** is dropped from the data structure but **still rendered**
  as a section in the published note — a manual fill-in completed in Obsidian.
  (The controlling location usually doesn't exist yet at faction-creation time;
  the location's guildhall branch later links back to the faction.)

## 8. Question flows

Each wizard branch sorts its questions into three buckets:

- **(1) Must** — GM-locked and required; the wizard won't generate without it.
  Shapes everything downstream.
- **(2) Can** — optional grounding; skipping falls back to free-text, a blank
  section, or a random/default.
- **(3) LLM** — generated under the locked answers, editable afterward via
  `reroll` / `set`.

### 8.1 Houses

Because `kind` *is* the political layer, houses splits into two sub-flows that
share a spine:

- **A — Great House** (apex; answers to no one)
- **B — Vassal / Lord** (Major Vassal, Minor Vassal, Individual Lord; sworn to a liege)

| # | Question | Bucket | Applies to | Notes |
|---|---|---|---|---|
| 1 | **House layer** (`kind`) | Must | all | routes the sub-flow |
| 2 | **Power base** (lord-type) | Must | all | the 6 types; seeds the Obstacle + pre-shapes the seat-location. **No random** — the GM has already read the logistics off their map |
| 2b | **Specifics** | Can | all | optional free-text naming the resource / route / holding (Extraction → "silver and salt"; Chokepoint → "the only bridge over the Ironwash") |
| 3a | **Brand** — what they're known for | Must | Great House | menu + custom: wealth / loyalty / martial might / piety / cunning / lineage… |
| 3b | **Liege** — who they're sworn to | Must | Vassal + Lord | faction picker → free-text fallback if no Great House exists yet |
| 4 | **Loyalty type** | Can | Vassal + Lord | enum, `0 = random`: reward / marriage / military / economic / shared-enemy / oath / secret. Always resolves to a value |
| 5 | **Ambition** (WOAC *Want*) | Can | all | optional GM seed; skip → LLM infers from layer + power base |
| 6 | **Leader** (NPC) | Can | all | NPC picker → skip = free-text or blank |
| 7 | **Allies** | Can | all | faction picker → skip = blank section |
| 8 | **Rivals** | Can | all | faction picker → skip = blank section |
| 9 | *Generate* | — | all | LLM fills the rest under the locked answers |

**LLM-filled (bucket 3), all editable afterward:** `name`, `public_description`,
`reputation`, `symbol_description`, the rest of WOAC — `obstacle` (auto-seeded by
the lord-type's vulnerability), `action`, `consequence` — plus
`sphere_of_influence` (scaled by the layer) and `resources_assets` (drawn from
the power base).

**Generation rules baked into the prompt:**

- *Obstacle* is pre-seeded by the chosen lord-type's built-in vulnerability
  (March → the crown wants the autonomy back; Extraction → the vein is running dry).
- *Great House* can't directly assault peers — it moves through proxies and vassals.
- *Vassal / Lord* feeds liege + loyalty type into the engine so the loyalty's
  fault line surfaces in the Obstacle.
- *Visible / hidden*: `public_description` is the claim; the leverage behind it
  shows up in `want` / `action`.

**Design intent:** the expected workflow is — read the region map, find where the
logistics support a barony, then generate the NPC and faction. Because the GM has
already identified the logistics, the power base is a strict Must with no random
option.

### 8.2 Establishments

Guild / Company / Criminal Syndicate is a **flat** flow — `kind` is flavor and
sets the legit-vs-illicit tone (Guild/Company keep the public/true gap narrow;
Syndicate widens it). The lord-type analog is a **control-type menu**, each
option carrying its own built-in vulnerability:

| Control type | Example | Vulnerability → Obstacle |
|---|---|---|
| **Craft / good** | smiths, alchemists, masons | a rival guild or cheap substitute undercuts the monopoly |
| **Service / force** | mercenaries, assassins, spies | only as good as the last job — a defeat or a betrayal |
| **Trade / transport** | caravans, shipping, brokers | a rival route, a new tariff, a revoked charter |
| **Vice / contraband** | smuggling, gambling, narcotics, theft | the law, a rival crew, a crackdown |
| **Knowledge / influence** | spymasters, fixers, money-lenders | a leaked secret, a debt called in |

| # | Question | Bucket | Notes |
|---|---|---|---|
| 1 | **Kind** (guild / company / syndicate) | Must | flavor + legit/illicit tone |
| 2 | **What they control** (control type) | Must | the menu above; seeds the Obstacle, like the lord-type does |
| 2b | **Specifics** | Can | optional free-text refining the control type (e.g. "iron, bronze, and steel smithing") |
| 3 | **Reach** | Must | local / regional / realm-spanning → grounds `sphere_of_influence` |
| 4 | **Patron / charter** | Can | optional faction picker — the house or power that charters/protects them; skip = none |
| 5 | **Ambition** (Want) | Can | optional GM seed |
| 6 | **Leader** (NPC) | Can | picker → skip |
| 7 | **Allies** | Can | picker → skip |
| 8 | **Rivals** | Can | picker → skip |
| 9 | *Generate* | — | LLM fills the rest |

**LLM-filled (bucket 3):** `name`, `public_description`, `reputation`,
`symbol_description`, the rest of WOAC — `obstacle` (auto-seeded by the
control-type's vulnerability), `action`, `consequence` — plus
`sphere_of_influence` (scaled by reach) and `resources_assets`.

**Generation rules:**

- *Obstacle* is pre-seeded by the control-type's built-in vulnerability.
- *Syndicate* widens the public-front vs true-racket gap; Guild/Company keep it narrow.
- *Patron/charter* (if linked) grounds the relationship — the establishment
  operates under that house's protection/charter, and that dependency is a fault line.

### 8.3 Religion

Temple / Cult is a **flat** flow — `kind` sets the tone: a **Temple** is a public
faith (narrow public/true gap); a **Cult** hides its real creed (wide gap). The
same god can be served openly by a Temple or darkly by a Cult. The power-base
analog is the **god's mandate** (menu + specifics):

| Mandate | Vulnerability → Obstacle |
|---|---|
| **Devotion & tribute** (worship, offerings) | donor fatigue, a richer rival temple |
| **Sacrifice** (blood, lives, valuables) | the supply of victims runs out, public backlash |
| **Conquest & conversion** (spread the faith) | resistance, a crusade against them |
| **Purity & law** (enforce a moral/ritual order) | schism over who's pure, purges |
| **Secret knowledge** (forbidden lore) | the secret leaks, rival seekers |
| **Cycle & nature** (death/rebirth, seasons, wilds) | a broken cycle, encroaching civilization |

| # | Question | Bucket | Notes |
|---|---|---|---|
| 1 | **Kind** (temple / cult) | Must | tone — public faith vs hidden cult |
| 2 | **God** | Must | picker → existing god entity; free-text fallback if none created yet |
| 3 | **What the god demands** (mandate) | Must | the menu above; kind colors it (Temple benevolent, Cult dark) |
| 3b | **Specifics** | Can | optional free-text (e.g. "midwinter blood offerings to ensure the harvest") |
| 4 | **Reach** | Must | local shrine / regional faith / realm-spanning church → `sphere_of_influence` |
| 5 | **Patron / charter** | Can | optional faction picker — a temple sanctioned by a house, or a cult's secret backer |
| 6 | **Ambition** (Want) | Can | optional GM seed (the mandate already shapes Want; this sharpens it) |
| 7 | **Leader** (NPC) | Can | high priest / cult leader picker → skip |
| 8 | **Allies** | Can | picker → skip |
| 9 | **Rivals** | Can | picker → skip |
| 10 | *Generate* | — | LLM fills the rest |

**LLM-filled (bucket 3):** `name`, `public_description`, `reputation`,
`symbol_description`, the rest of WOAC — `obstacle` (seeded by the mandate's
vulnerability and sharpened by kind), `action`, `consequence` — plus
`sphere_of_influence` (scaled by reach) and `resources_assets`.

**Generation rules:**

- The **god picker** pulls the deity's own domain/portfolio into the prompt so the
  mandate stays consistent with who they worship.
- *Obstacle* is seeded by the mandate's vulnerability and sharpened by kind
  (Cult → exposure & suppression; Temple → schism & rival faiths).
- *Visible / hidden*: Cult widens the public-creed vs true-creed gap; Temple keeps
  them aligned.

## 9. Reuse of the location-wizard pattern

The faction wizard is built on the existing host-agnostic wizard framework
(`WizardStep` / `Wizard` / `WizardTransition`; type-erased accumulator), exactly
as the location wizard is:

- A `FactionWizardData` accumulator + per-step logic.
- A `FactionWizard` impl registered in the wizard registry.
- Branch on **category** at step 1; route **houses** further by **kind**.
- The one-shot lane (`create faction <prompt>`) is retained, unchanged.
- `finalize()` builds a `FactionDraft` and hands off to the faction editor —
  like `editor.set_location(...)`.

## 10. Out of scope

- **Migration.** Factions saved under the old 10-kind enum are **not** migrated —
  the new scheme is a breaking change (solo project; test data is wiped manually
  between releases).
