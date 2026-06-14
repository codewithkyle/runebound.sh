## Reroll Enhancement Plan

### Problem
- The standalone `reroll` command ignores any text the user appends (e.g., `reroll make her older`).
- Editors who want to steer the reroll must fall back to the verbose `npc reroll <field> [prompt]` / `location reroll` / `faction reroll` commands.
- Users expect the quick `reroll` command to respect extra guidance, just like the scoped reroll commands do.

### Desired Behavior
- Allow `reroll` to accept an optional prompt fragment.
- Combine that fragment with the draft's saved `seed_prompt`, mirroring the logic in `merge_seed_and_reroll_prompt` so both contexts reach the generator.
- Surface the merged prompt in telemetry/logs so we can debug prompt chains.

### Implementation Notes
1. **Command Parsing**
   - Update the desktop router's `reroll` handler to capture trailing text (similar to the field-specific reroll commands) and pass it as `Option<String>`.
   - Preserve backwards compatibility: `reroll` with no prompt stays valid.
2. **State + Prompt Merging**
   - Extend `reroll_current_npc/location/faction` to accept an optional prompt override.
   - Reuse `merge_seed_and_reroll_prompt` (or extract a shared helper) so `seed_prompt` + `reroll` hint form a single string.
3. **Generation Calls**
   - Thread the merged prompt into `Generate*SeedInput` so the AI sees the additional guidance.
4. **UI Feedback**
   - Update spinner labels / history entries to show when a custom prompt was supplied (e.g., `rerolling npc (custom prompt)`).
5. **Telemetry & Tests**
   - Log the merged prompt for observability.
   - Add integration tests ensuring `reroll` with and without prompts results in different seed payloads.

### Open Questions
- Should the prompt fragment be appended or replace the original seed? (Current proposal: append, clearly separating the two.)
- Do we need rate limits or validation to avoid excessively long merged prompts?
- Should the backend store the last reroll prompt so repeated `reroll` commands stack?

Documented 2026-06-14 to guide the Phase 3/4 backlog.
