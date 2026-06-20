// Auto-generated from the Rust types in `runebound-models` via ts-rs.
// Do not edit by hand. Regenerate with:
//   UPDATE_MODELS=1 cargo test -p runebound-models

export type NpcDraft = { id: string, seed_prompt: string | null, name: string, slug: string, race: string, occupation: string, sex: string, age: string, height: string, weight_lbs: string, background: string, want_need: string, secret_obstacle: string, carrying: Array<string>, location: string, };

export type LocationDraft = { id: string, seed_prompt: string | null, name: string, slug: string, vault_path: string, kind_type: string, kind_custom: string | null, visual_description: string, history_background: string, exports: Array<string>, tone: string, authority: string, danger_level: string, current_tension: string, 
/**
 * The location this one stands within (a guildhall's containing place). Empty
 * when there is no anchor; published as a `[[wikilink]]`.
 */
location: string, };

export type FactionDraft = { id: string, seed_prompt: string | null, name: string, slug: string, vault_path: string, kind_type: string, public_description: string, reputation: string, symbol_description: string, want: string, obstacle: string, action: string, consequence: string, 
/**
 * Was `leadership`. An NPC link name or free text; wizard-picked or left blank,
 * never LLM-generated (D3).
 */
leader: string, sphere_of_influence: string, resources_assets: Array<string>, 
/**
 * Picker-linked or left blank; never LLM-generated (D3/§7).
 */
allies: Array<string>, 
/**
 * Picker-linked or left blank; never LLM-generated (D3/§7).
 */
rivals_enemies: Array<string>, 
/**
 * Houses Vassal/Lord only — the faction this one is sworn to. Picker or free
 * text, never LLM.
 */
liege: string | null, 
/**
 * Houses Vassal/Lord only — one of `LOYALTY_TYPES`. Enum-picked or random,
 * never LLM.
 */
loyalty_type: string | null, };

export type ItemDraft = { id: string, seed_prompt: string | null, name: string, slug: string, vault_path: string, category: string, rarity: string, attunement: string, materials: Array<string>, appearance: string, abilities: string, drawbacks: string, history: string, value: string, location: string, };

export type EventDraft = { id: string, seed_prompt: string | null, name: string, slug: string, body: string, };

export type GodDraft = { id: string, seed_prompt: string | null, name: string, slug: string, vault_path: string, epithet: string, rank: string, rank_custom: string | null, alignment: string, domains: Array<string>, symbol: string, appearance: string, dogma: string, realm: string, worshippers: string, clergy: string, allies: Array<string>, rivals: Array<string>, };

export type DungeonBeat = { function: string, content_type: string, idea: string, player_goals: string, lever: string, loot: string | null, design_note: string, overlay: string | null, factions: boolean, };

export type DungeonDraft = { id: string, seed_prompt: string | null, name: string, slug: string, vault_path: string, location: string, story: string, premise: string, topology: string, tone: string, twist: string, beats: Array<DungeonBeat>, };

export type EntityCardRow = { label: string, value: string, };

export type InlineNode = { "kind": "text", text: string, } | { "kind": "command_ref", label: string, command: string, } | { "kind": "emphasis", text: string, } | { "kind": "strong", text: string, } | { "kind": "code", text: string, };

export type StatusTone = "success" | "info" | "warning" | "error";

export type SpinnerState = "running" | "success" | "error";

export type OutputBlock = { "kind": "heading", level: number, text: string, } | { "kind": "paragraph", inlines: Array<InlineNode>, } | { "kind": "list", items: Array<Array<InlineNode>>, } | { "kind": "code", language: string | null, text: string, } | { "kind": "status", tone: StatusTone, text: string, } | { "kind": "spinner", state: SpinnerState, text: string, } | { "kind": "entity_card", title: string, rows: Array<EntityCardRow>, } | { "kind": "image", src: string, alt: string, };

export type OutputDoc = { blocks: Array<OutputBlock>, };

export type OutputSegmentKind = "text" | "error";

export type OutputSegment = { kind: OutputSegmentKind, text: string, command_ref: string | null, };

export type CommandClientEvent = { "kind": "load_npc_draft_with_card", draft: NpcDraft, entity_card: OutputDoc, } | { "kind": "load_location_draft_with_card", draft: LocationDraft, entity_card: OutputDoc, } | { "kind": "load_faction_draft_with_card", draft: FactionDraft, entity_card: OutputDoc, } | { "kind": "load_item_draft_with_card", draft: ItemDraft, entity_card: OutputDoc, } | { "kind": "load_event_draft_with_card", draft: EventDraft, entity_card: OutputDoc, } | { "kind": "load_god_draft_with_card", draft: GodDraft, entity_card: OutputDoc, } | { "kind": "load_dungeon_draft_with_card", draft: DungeonDraft, entity_card: OutputDoc, } | { "kind": "clear_drafts" } | { "kind": "clear_terminal", clear_history: boolean, } | { "kind": "exit_requested" };

export type WizardView = { 
/**
 * Active wizard id, e.g. "dungeon".
 */
id: string, 
/**
 * Current step id, e.g. "plan_review".
 */
step_id: string, 
/**
 * Spinner label to show when the user submits from this step (None = instant).
 */
awaiting_llm_label: string | null, };

export type CommandResponse = { ok: boolean, output: string, error: string | null, exit_code: number, segments: Array<OutputSegment>, output_doc: OutputDoc | null, client_event: CommandClientEvent | null, 
/**
 * Set when a multi-step wizard is active, so the frontend can drive the
 * spinner from a structured signal instead of matching prompt text.
 */
wizard: WizardView | null, };
