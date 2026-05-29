use serde::{Deserialize, Serialize};

use crate::parser::ParsedMedia;
use crate::providers::ShowCandidate;

// ---- Settings ----

#[derive(Serialize)]
pub struct PermissionCheck {
    pub label: String,
    pub exists: bool,
    pub read: bool,
    pub write: Option<bool>,
}

#[derive(Serialize)]
pub struct SettingsResponse {
    pub input_roots: Vec<String>,
    pub tv_output_root: String,
    pub anime_output_root: String,
    pub film_output_root: String,
    pub copy_rate_limit_mbps: String,
    pub checks: Vec<PermissionCheck>,
}

#[derive(Deserialize)]
pub struct SettingsPayload {
    #[serde(default)]
    pub input_roots: Vec<String>,
    #[serde(default)]
    pub tv_output_root: String,
    #[serde(default)]
    pub anime_output_root: String,
    #[serde(default)]
    pub film_output_root: String,
    #[serde(default = "default_rate")]
    pub copy_rate_limit_mbps: String,
}

fn default_rate() -> String {
    "15".to_string()
}

// ---- Browse ----

#[derive(Serialize)]
pub struct BrowseEntry {
    pub name: String,
    pub relative_path: String,
    pub is_dir: bool,
    pub is_video: bool,
    pub size: Option<i64>,
    pub size_human: String,
    pub status: Option<String>,
    pub status_key: String,
    pub manual_status: String,
    pub latest_import_result: Option<String>,
    pub source_count: usize,
}

#[derive(Serialize)]
pub struct BrowseRootInfo {
    pub id: i64,
    pub path: String,
}

#[derive(Serialize)]
pub struct BrowseResponse {
    pub roots: Vec<BrowseRootInfo>,
    pub active_root: Option<BrowseRootInfo>,
    pub current_path: String,
    pub parent_path: String,
    pub entries: Vec<BrowseEntry>,
    pub error: Option<String>,
}

// ---- Match ----

#[derive(Deserialize)]
pub struct MatchPayload {
    pub root_id: i64,
    pub media_type: String,
    #[serde(default)]
    pub selected: Vec<String>,
}

#[derive(Serialize)]
pub struct MatchRow {
    pub source_path: String,
    pub source_name: String,
    pub parsed: ParsedMedia,
    pub show_title: String,
    pub show_year: Option<i64>,
    pub season_number: i64,
    pub episode_number: i64,
    pub episode_title: String,
    pub quality: String,
    pub provider: String,
    pub provider_show_id: String,
    pub candidates: Vec<ShowCandidate>,
    pub metadata_error: Option<String>,
}

#[derive(Serialize)]
pub struct MatchResponse {
    pub media_type: String,
    pub output_root: Option<String>,
    pub rows: Vec<MatchRow>,
}

// ---- Import / Preview ----

#[derive(Deserialize, Clone)]
pub struct ImportItem {
    pub source_path: String,
    pub show_title: String,
    pub show_year: Option<i64>,
    #[serde(default)]
    pub season_number: i64,
    #[serde(default)]
    pub episode_number: i64,
    #[serde(default)]
    pub episode_title: String,
    pub quality: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub provider_show_id: Option<String>,
}

#[derive(Deserialize)]
pub struct ImportBatch {
    pub media_type: String,
    pub action: String,
    pub conflict_policy: String,
    #[serde(default)]
    pub items: Vec<ImportItem>,
}

// ---- Source status ----

#[derive(Deserialize)]
pub struct SourceStatusPayload {
    pub status: String,
    pub root_id: Option<i64>,
    #[serde(default)]
    pub selected: Vec<String>,
    pub source_path: Option<String>,
}

// ---- Search ----

#[derive(Deserialize)]
pub struct SearchQuery {
    pub media_type: String,
    pub q: String,
}

#[derive(Deserialize)]
pub struct EpisodesQuery {
    pub media_type: String,
    pub provider_show_id: String,
}

// ---- Folders ----

#[derive(Deserialize)]
pub struct FoldersQuery {
    #[serde(default = "root_default")]
    pub path: String,
}

fn root_default() -> String {
    "/".to_string()
}

#[derive(Serialize)]
pub struct FolderEntry {
    pub name: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct FoldersResponse {
    pub path: String,
    pub parent: Option<String>,
    pub folders: Vec<FolderEntry>,
    pub roots: Vec<String>,
}

#[derive(Deserialize)]
pub struct BrowseQuery {
    pub root_id: Option<i64>,
    #[serde(default)]
    pub path: String,
}
