# Addendum: NPC and Faction Publish Templates

## NPC Publish Template

For `type = "npc"`, generate readable Markdown from the actual NPC TOML fields.

### Source Fields

Supported NPC fields:

```txt
type
id
slug
name
race
occupation
sex
age
height
weight_lbs
background
want_need
secret_obstacle
carrying
location
created_at
updated_at
```

### Fields Hidden From Published Markdown

The following fields should not be shown in the published output:

```txt
type
id
slug
created_at
updated_at
```

### NPC Output Template

```md
## {name}

> [!summary]
> {name} is a {age}-year-old {race} {occupation} currently associated with {location}.

### At a Glance

| Field | Details |
|---|---|
| Race | {race} |
| Occupation | {occupation} |
| Sex | {sex} |
| Age | {age} |
| Height | {height} |
| Weight | {weight_lbs} lbs |
| Location | {location} |

### Background

{background}

### Want / Need

{want_need}

### Secret Obstacle

{secret_obstacle}

### Carrying

- {carrying[0]}
- {carrying[1]}
- {carrying[2]}
```

### NPC Formatting Rules

* Omit any empty or missing fields.
* Omit the summary callout if there is not enough data to form a useful sentence.
* If `weight_lbs` is present, render it as `{weight_lbs} lbs`.
* If `carrying` is empty or missing, omit the `### Carrying` section.
* Preserve the exact text from long-form fields such as `background`, `want_need`, and `secret_obstacle`.

### NPC Example Output

```md
%% runebound:published:start id="npc_20260614031230995" source_hash="abc123" %%

## Bartholomew Finch

> [!summary]
> Bartholomew Finch is a 28-year-old Human Town Guard currently associated with Stormborough Holdfast.

### At a Glance

| Field | Details |
|---|---|
| Race | Human |
| Occupation | Town Guard |
| Sex | male |
| Age | 28 |
| Height | 5'6" |
| Weight | 165 lbs |
| Location | Stormborough Holdfast |

### Background

Bartholomew grew up under the shadow of his older, more accomplished brother, a renowned knight. Joining the city watch was meant to be an escape from that constant comparison, but he’s found himself constantly striving for recognition from Captain Elmsworth, who views him as clumsy and unreliable. He tries very hard, meticulously following orders, though often with disastrous results.

### Want / Need

To earn the respect and approval of Captain Elmsworth, demonstrating competence and bravery.

### Secret Obstacle

Crippling self-doubt that manifests as physical clumsiness under pressure.

### Carrying

- Rusty spear
- Leather breastplate
- Whistle (for summoning reinforcements)
- Small pouch of dried berries

%% runebound:published:end %%
```

---

## Faction Publish Template

For `type = "faction"`, generate readable Markdown from the actual faction TOML fields.

### Source Fields

Supported faction fields:

```txt
type
id
slug
name
kind_type
public_description
true_agenda
methods
leadership
headquarters
sphere_of_influence
resources_assets
allies
rivals_enemies
reputation
current_tension
goals_short_term
goals_long_term
symbol_description
created_at
updated_at
```

### Fields Hidden From Published Markdown

The following fields should not be shown in the published output:

```txt
type
id
slug
created_at
updated_at
```

### Faction Output Template

```md
## {name}

> [!summary]
> {name} is a {kind_type} led by {leadership}.

### At a Glance

| Field | Details |
|---|---|
| Type | {kind_type} |
| Leadership | {leadership} |
| Headquarters | {headquarters} |
| Sphere of Influence | {sphere_of_influence} |
| Reputation | {reputation} |

### Public Description

{public_description}

### True Agenda

{true_agenda}

### Methods

{methods}

### Resources / Assets

{resources_assets}

### Allies

- {allies[0]}
- {allies[1]}

### Rivals / Enemies

- {rivals_enemies[0]}
- {rivals_enemies[1]}

### Short-Term Goals

- {goals_short_term[0]}
- {goals_short_term[1]}

### Long-Term Goals

- {goals_long_term[0]}
- {goals_long_term[1]}

### Current Tension

{current_tension}

### Symbol

{symbol_description}
```

### Faction Formatting Rules

* Omit any empty or missing fields.
* Omit the summary callout if there is not enough data to form a useful sentence.
* Render `allies`, `rivals_enemies`, `goals_short_term`, and `goals_long_term` as bullet lists.
* Render `methods` and `resources_assets` as paragraphs, not lists, because they are stored as strings.
* Preserve the exact text from long-form fields such as `public_description`, `true_agenda`, `methods`, and `current_tension`.

### Faction Example Output

```md
%% runebound:published:start id="fac_20260614053214441" source_hash="def456" %%

## Chartwright's Concord

> [!summary]
> Chartwright's Concord is a guild led by Grand Archivist Lyra Meadowlight, a seasoned cartographer known for her meticulous work and calm demeanor.

### At a Glance

| Field | Details |
|---|---|
| Type | guild |
| Leadership | Grand Archivist Lyra Meadowlight, a seasoned cartographer known for her meticulous work and calm demeanor. |
| Headquarters | The Cartographer's Archive, a sprawling structure built into the plateau with extensive libraries and observation chambers. |
| Sphere of Influence | Aethelgard's Rest and a fluctuating radius within the Whispering Bluffs. |
| Reputation | Respected, relied upon, but viewed with a degree of cautiousness due to the unpredictable nature of their work. |

### Public Description

The Guild of Cartographers maintains the essential service of charting Aethelgard’s Rest and surrounding bluffs. Their maps, though often temporary, are vital for safe travel and trade. They offer apprenticeships to promising individuals showing aptitude in observation and documentation.

### True Agenda

To understand and ultimately control the planar instability affecting the region, preserving Aethelgard's Rest. They believe controlling the Bluffs is key to long term security. Their efforts are veiled behind a public facade of impartial mapping.

### Methods

Meticulous observation, detailed record-keeping, advanced surveying techniques employing planar resonance sensors (rudimentary). They subtly influence town policy to prioritize mapping initiatives and allocate resources. Information is carefully guarded and selectively shared.

### Resources / Assets

Extensive cartographic archives, specialized surveying equipment (resonators), bioluminescent pigments for map inks, influence within Aethelgard’s Rest council.

### Allies

- Scholars interested in planar anomalies
- Local herbalists seeking rare pigments.

### Rivals / Enemies

- Those seeking to exploit planar shifts for personal gain
- Independent surveyors who challenge the Guild's authority.

### Short-Term Goals

- Verify and update current cartographic records.
- Establish safe travel routes through the Bluffs despite the instability.

### Long-Term Goals

- Achieve a comprehensive understanding of the Whispering Bluffs' planar influence.
- Develop methods for predicting or mitigating the land’s unpredictable shifts.

### Current Tension

The land’s increasingly erratic shifts are disrupting established routes and rendering older maps useless, causing anxiety among the townsfolk who depend on them.

### Symbol

The guild’s banner depicts a shifting spiral of turquoise, emerald, and ochre representing the ever-changing landscape they map.

%% runebound:published:end %%
```

---

## Updated Acceptance Criteria

Add these acceptance criteria to the publish ticket:

* NPC publishing supports the actual NPC schema fields: `race`, `occupation`, `sex`, `age`, `height`, `weight_lbs`, `background`, `want_need`, `secret_obstacle`, `carrying`, and `location`.
* Faction publishing supports the actual faction schema fields: `kind_type`, `public_description`, `true_agenda`, `methods`, `leadership`, `headquarters`, `sphere_of_influence`, `resources_assets`, `allies`, `rivals_enemies`, `reputation`, `current_tension`, `goals_short_term`, `goals_long_term`, and `symbol_description`.
* NPC and faction published output does not show internal fields: `type`, `id`, `slug`, `created_at`, or `updated_at`.
* Array fields are rendered as bullet lists.
* String fields are rendered as paragraphs or table values depending on the template.
* Missing optional fields are omitted cleanly without leaving blank headings, blank table rows, or placeholder text.
* Re-running publish replaces the existing published section for NPCs and factions instead of appending duplicates.
