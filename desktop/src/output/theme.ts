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
    return "rb-spinner rb-spinner-state-error";
  }
  if (state === "success") {
    return "rb-spinner rb-spinner-state-success";
  }
  return "rb-spinner rb-spinner-state-running";
}

export function spinnerTextClass(state: SpinnerState): string {
  if (state === "error") {
    return "rb-spinner-text rb-spinner-text-error";
  }
  if (state === "success") {
    return "rb-spinner-text rb-spinner-text-success";
  }
  return "rb-spinner-text rb-spinner-text-running";
}

export const commandRefClass = "rb-command-ref";
