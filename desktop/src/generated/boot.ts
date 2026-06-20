// Auto-generated from the Rust types in `crate::boot` via ts-rs. Do not edit by
// hand. Regenerate with:
//   UPDATE_MODELS=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml

export type BootTone = "success" | "warning" | "error";

export type BootTaskInfo = { id: string, 
/**
 * Fun, in-world label shown next to the spinner.
 */
label: string, };

export type BootPlan = { 
/**
 * When true the app is not configured yet; the frontend skips the spinners
 * and shows the first-time setup message instead.
 */
needs_setup: boolean, tasks: Array<BootTaskInfo>, };

export type BootTaskResult = { ok: boolean, 
/**
 * Status tone for the finished spinner.
 */
tone: BootTone, detail: string, };
