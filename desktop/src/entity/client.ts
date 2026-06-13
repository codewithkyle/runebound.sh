import { invoke } from "@tauri-apps/api/core";

export type EntityType = "npc" | "location";

export type EntitySuggestion = {
  entity_type: EntityType;
  name: string;
  slug: string;
};

export type EntityDetails = {
  entity_type: EntityType;
  name: string;
  slug: string;
  race?: string | null;
  sex?: string | null;
  location?: string | null;
  vault_path: string;
};

export async function searchEntities(query: string, limit = 8): Promise<EntitySuggestion[]> {
  return invoke<EntitySuggestion[]>("search_entities", { query, limit });
}

export async function resolveEntity(input: string): Promise<EntityDetails | null> {
  return invoke<EntityDetails | null>("resolve_entity", { input });
}
