use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    pub race: String,
    pub occupation: String,
    pub sex: String,
    pub age: String,
    pub height: String,
    pub weight_lbs: String,
    pub background: String,
    pub want_need: String,
    pub secret_obstacle: String,
    #[serde(default)]
    pub carrying: Vec<String>,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    pub slug: String,
    pub vault_path: String,
    pub kind_type: String,
    #[serde(default)]
    pub kind_custom: Option<String>,
    pub visual_description: String,
    pub history_background: String,
    #[serde(default)]
    pub exports: Vec<String>,
    pub tone: String,
    pub authority: String,
    pub danger_level: String,
    pub current_tension: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactionDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    pub slug: String,
    pub vault_path: String,
    pub kind_type: String,
    #[serde(default)]
    pub kind_custom: Option<String>,
    pub public_description: String,
    pub true_agenda: String,
    pub methods: String,
    pub leadership: String,
    pub headquarters: String,
    pub sphere_of_influence: String,
    pub resources_assets: String,
    #[serde(default)]
    pub allies: Vec<String>,
    #[serde(default)]
    pub rivals_enemies: Vec<String>,
    pub reputation: String,
    pub current_tension: String,
    #[serde(default)]
    pub goals_short_term: Vec<String>,
    #[serde(default)]
    pub goals_long_term: Vec<String>,
    pub symbol_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub race: String,
    pub occupation: String,
    pub sex: String,
    pub age: String,
    pub height: String,
    pub weight_lbs: String,
    pub background: String,
    pub want_need: String,
    pub secret_obstacle: String,
    pub carrying: Vec<String>,
    pub location: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub kind_type: String,
    #[serde(default)]
    pub kind_custom: Option<String>,
    pub visual_description: String,
    pub history_background: String,
    pub exports: Vec<String>,
    pub tone: String,
    pub authority: String,
    pub danger_level: String,
    pub current_tension: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactionFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub kind_type: String,
    #[serde(default)]
    pub kind_custom: Option<String>,
    pub public_description: String,
    pub true_agenda: String,
    pub methods: String,
    pub leadership: String,
    pub headquarters: String,
    pub sphere_of_influence: String,
    pub resources_assets: String,
    pub allies: Vec<String>,
    pub rivals_enemies: Vec<String>,
    pub reputation: String,
    pub current_tension: String,
    pub goals_short_term: Vec<String>,
    pub goals_long_term: Vec<String>,
    pub symbol_description: String,
    pub created_at: String,
    pub updated_at: String,
}