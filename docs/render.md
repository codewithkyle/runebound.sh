# Rendering Architecture Guide

This file is a handoff guide for future agents and contributors.

## Purpose

- Explain how output rendering works end-to-end.
- Define how to add new output safely.
- Prevent regressions from ad hoc text formatting.

## Current Architecture

### Backend output contract (Rust)

- Commands return `CommandResponse` from `core/src/command.rs`.
- Preferred structured field: `output_doc`.
- Compatibility fields still exist: `output`, `error`, `segments`.

### Structured output model

- Rust types live in `core/src/output.rs`.
- Core model:
  - `OutputDoc { blocks: Vec<OutputBlock> }`
  - `OutputBlock` variants: `heading`, `paragraph`, `list`, `code`, `status`, `spinner`
  - `InlineNode` variants include `command_ref` for clickable commands

### Frontend render path

- Parser/adapter for plain text -> structured blocks: `desktop/src/output/markdown.ts`
- UI renderer: `desktop/src/output/renderer.tsx`
- Semantic class mapping: `desktop/src/output/theme.ts`
- Styling tokens/classes: `desktop/src/index.css`

## Golden Rules

1. Prefer explicit structured output from backend (`output_doc`) over plain strings.
2. If text output is used, keep markdown-inspired structure stable (`##`, lists, inline code).
3. Actionable commands shown to users must be explicit `command_ref` where possible.
4. Avoid adding one-off regex render hacks in `App.tsx`.
5. Keep command definitions synchronized with manifest/help/autocomplete.

## How Clickable Commands Work

- Best path: emit `InlineNode::CommandRef` in backend output.
- Fallback path: parser in `markdown.ts` detects command-like segments and resolves against manifest metadata.
- Command resolution rules live in `desktop/src/App.tsx` (`resolveClickableCommandTarget` + command metadata map).

## Adding New Output (Recommended Workflow)

1. **Backend first**: add/extend command output in `core/src/command.rs`.
2. Build an `OutputDoc` using helpers from `core/src/output.rs`.
3. Include explicit `command_ref` in actionable text.
4. Keep plain `output` text meaningful during transition.
5. Verify frontend renders correctly with `OutputRenderer`.

## Adding a New Command Safely

When adding/removing/changing a command:

- Update backend runtime handling (`core/src/command.rs` and/or `desktop/src-tauri/src/router.rs`).
- Update manifest in `core/src/command_manifest.rs`.
- Ensure help output and examples are updated.
- Ensure actionable command text is clickable.
- Verify keyboard UX invariants (`Enter`, `Tab`, arrows, `Ctrl+C`).
- Keep Clap definitions synchronized only for Clap-managed commands during transition.
- Run `make build`.

## Spinner, Error, and Info Semantics

- Spinner should render with semantic spinner blocks/styles (`spinner` block + theme classes).
- Errors should use `status(error)` semantics where possible.
- Helper guidance should use `status(info)` semantics.

## Current Known Constraints

- Some onboarding/editor responses still rely on markdown-inspired text parsing fallback.
- Goal state is full explicit `OutputDoc` emission for all command paths.

## Suggested Next Increment

- Expand backend `output_doc` coverage for routed desktop commands.
- Emit explicit `OutputDoc` directly instead of passing raw strings to parser.
- Reduce parser fallback to compatibility only.

## Verification Checklist Before Merging

- `make build` passes.
- Startup banner/help commands clickable.
- Setup help commands clickable and left-aligned.
- `status`, `config show`, `config test` render with stable structure.
- No new ad hoc output regex added in app rendering path.
