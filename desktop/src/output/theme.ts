import type { SpinnerState, StatusTone } from "./types";

export function statusClass(tone: StatusTone): string {
  if (tone === "error") {
    return "rb-status rb-status-error";
  }
  if (tone === "warning") {
    return "rb-status rb-status-warning";
  }
  if (tone === "success") {
    return "rb-status rb-status-success";
  }
  return "rb-status rb-status-info";
}

export function spinnerClass(state: SpinnerState): string {
  if (state === "error") {
    return "rb-spinner rb-spinner-error";
  }
  if (state === "success") {
    return "rb-spinner rb-spinner-success";
  }
  return "rb-spinner rb-spinner-running";
}

export const commandRefClass = "rb-command-ref";
