import { invoke } from "@tauri-apps/api/core";

export type NpcSeed = {
  name: string;
  race: string;
  occupation: string;
  sex: "male" | "female";
  age: string;
  height: string;
  weight_lbs: string;
  background: string;
  want_need: string;
  secret_obstacle: string;
  carrying: string[];
};

export type EnsureLocationResult = {
  name: string;
  slug: string;
  vault_path: string;
  created_file: boolean;
  created_record: boolean;
};

export type SaveNpcDraftInput = {
  id: string;
  name: string;
  race: string;
  occupation: string;
  sex: "male" | "female";
  age: string;
  height: string;
  weight_lbs: string;
  background: string;
  want_need: string;
  secret_obstacle: string;
  carrying: string[];
  location: string;
};

export type SaveNpcDraftResult = {
  id: string;
  slug: string;
  vault_path: string;
  created_at: string;
  updated_at: string;
};

export type NpcRerollContext = {
  name: string;
  race: string;
  occupation: string;
  sex: "male" | "female";
  age: string;
  height: string;
  weight_lbs: string;
  background: string;
  want_need: string;
  secret_obstacle: string;
  carrying: string[];
  location: string;
};

export type RerollNpcFieldInput = {
  field: string;
  prompt?: string | null;
  npc: NpcRerollContext;
};

export type RerollNpcFieldResult = {
  field: string;
  value?: string | null;
  carrying?: string[] | null;
};

export async function generateNpcSeed(prompt?: string): Promise<NpcSeed> {
  return invoke<NpcSeed>("generate_npc_seed", {
    input: {
      prompt: prompt ?? null
    }
  });
}

export async function ensureLocationExists(name: string): Promise<EnsureLocationResult> {
  return invoke<EnsureLocationResult>("ensure_location_exists", {
    input: { name }
  });
}

export async function saveNpcDraft(input: SaveNpcDraftInput): Promise<SaveNpcDraftResult> {
  return invoke<SaveNpcDraftResult>("save_npc_draft", {
    input
  });
}

export async function rerollNpcField(input: RerollNpcFieldInput): Promise<RerollNpcFieldResult> {
  return invoke<RerollNpcFieldResult>("reroll_npc_field", {
    input
  });
}
