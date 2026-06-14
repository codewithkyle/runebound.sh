import { invoke } from "@tauri-apps/api/core";

export type EntityType = "npc" | "location";

export type EntitySuggestion = {
  entity_type: EntityType;
  name: string;
  slug: string;
};

export type EntityDetails = {
  id: string;
  entity_type: EntityType;
  name: string;
  slug: string;
  race?: string | null;
  occupation?: string | null;
  sex?: string | null;
  age?: string | null;
  height?: string | null;
  weight_lbs?: string | null;
  background?: string | null;
  want_need?: string | null;
  secret_obstacle?: string | null;
  carrying?: string[] | null;
  location?: string | null;
  vault_path: string;
  created_at?: string | null;
};

export type SaveLocationDraftInput = {
  id: string;
  name: string;
  slug: string;
  vault_path: string;
};

export type SaveLocationDraftResult = {
  id: string;
  slug: string;
  vault_path: string;
  created_at: string;
  updated_at: string;
};

export type SoftDeleteEntityInput = {
  target: string;
};

export type SoftDeleteEntityResult = {
  entity_type: EntityType;
  id: string;
  name: string;
  slug: string;
  trash_vault_path: string;
};

export type UndoSoftDeleteResult = {
  entity_type: EntityType;
  id: string;
  name: string;
  slug: string;
  vault_path: string;
};

export async function searchEntities(query: string, limit = 8): Promise<EntitySuggestion[]> {
  return invoke<EntitySuggestion[]>("search_entities", { query, limit });
}

export async function resolveEntity(input: string): Promise<EntityDetails | null> {
  return invoke<EntityDetails | null>("resolve_entity", { input });
}

export async function saveLocationDraft(input: SaveLocationDraftInput): Promise<SaveLocationDraftResult> {
  return invoke<SaveLocationDraftResult>("save_location_draft", { input });
}

export async function softDeleteEntity(input: SoftDeleteEntityInput): Promise<SoftDeleteEntityResult> {
  return invoke<SoftDeleteEntityResult>("soft_delete_entity", { input });
}

export async function undoLastSoftDelete(): Promise<UndoSoftDeleteResult> {
  return invoke<UndoSoftDeleteResult>("undo_last_soft_delete");
}
