import { invoke } from "@tauri-apps/api/core";

export type EntityType = "npc" | "location";

export type EntitySuggestion = {
  entity_type: EntityType;
  name: string;
  slug: string;
};

export async function searchEntities(query: string, limit = 8): Promise<EntitySuggestion[]> {
  return invoke<EntitySuggestion[]>("search_entities", { query, limit });
}
