use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::assets::static_handler;
use crate::db::ImportRow;
use crate::error::{AppError, AppResult};
use crate::filesystem::{
    canonical_or_normalized, expand_grouped, expand_source_files, is_relative_to, list_directory,
};
use crate::formatting::human_file_size;
use crate::importer::{preview_import, ImportRequest};
use crate::models::*;
use crate::parser::{parse_film_filename, ParsedMedia};
use crate::providers::{EpisodeCandidate, ShowCandidate};
use crate::state::{
    is_valid_action, is_valid_conflict_policy, is_valid_media_type, AppState, MEDIA_TYPES,
    PICKER_ROOTS,
};

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/api/settings", get(get_settings).put(save_settings))
        .route("/api/browse", get(browse))
        .route("/api/match", post(match_route))
        .route("/api/preview", post(preview))
        .route("/api/import-jobs", get(list_import_jobs).post(start_import_job))
        .route("/api/import-jobs/clear", post(clear_import_jobs))
        .route("/api/import-jobs/:id", get(get_import_job))
        .route("/api/import-jobs/:id/cancel", post(cancel_import_job))
        .route("/api/import-jobs/:id/items/:index/cancel", post(cancel_import_item))
        .route("/api/import-jobs/:id/results", get(import_job_results))
        .route("/api/library", get(library))
        .route("/api/library/rescan", post(rescan_library))
        .route("/api/history", get(history))
        .route("/api/search", get(search))
        .route("/api/episodes", get(episodes))
        .route("/api/source-status", post(source_status))
        .route("/api/folders", get(folders))
        .with_state(state);

    api.fallback(static_handler)
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

async fn get_settings(State(state): State<AppState>) -> Json<SettingsResponse> {
    let roots: Vec<String> = state.db.list_input_roots().into_iter().map(|r| r.path).collect();
    let output_roots = state.output_roots();
    let checks = settings_checks(&roots, &output_roots);
    Json(SettingsResponse {
        input_roots: roots,
        tv_output_root: state.db.get_setting("tv_output_root", ""),
        anime_output_root: state.db.get_setting("anime_output_root", ""),
        film_output_root: state.db.get_setting("film_output_root", ""),
        copy_rate_limit_mbps: state.db.get_setting("copy_rate_limit_mbps", "15"),
        checks,
    })
}

async fn save_settings(
    State(state): State<AppState>,
    Json(payload): Json<SettingsPayload>,
) -> Json<Value> {
    let roots: Vec<String> = payload
        .input_roots
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| normalize_path(line))
        .collect();
    state.db.replace_input_roots(&roots);
    state.db.set_setting(
        "tv_output_root",
        &normalize_optional(&payload.tv_output_root),
    );
    state.db.set_setting(
        "anime_output_root",
        &normalize_optional(&payload.anime_output_root),
    );
    state.db.set_setting(
        "film_output_root",
        &normalize_optional(&payload.film_output_root),
    );
    state.db.set_setting(
        "copy_rate_limit_mbps",
        &normalize_copy_rate_limit(&payload.copy_rate_limit_mbps),
    );
    Json(json!({ "status": "ok" }))
}

fn settings_checks(input_roots: &[String], output_roots: &HashMap<String, PathBuf>) -> Vec<PermissionCheck> {
    let mut checks = Vec::new();
    for root in input_roots {
        let path = PathBuf::from(root);
        checks.push(PermissionCheck {
            label: format!("Input: {}", path.display()),
            exists: path.exists(),
            read: path.is_dir() && os_access(&path, false),
            write: None,
        });
    }
    for &media_type in MEDIA_TYPES {
        if let Some(path) = output_roots.get(media_type) {
            checks.push(PermissionCheck {
                label: format!("{} output: {}", title_word(media_type), path.display()),
                exists: path.exists(),
                read: path.is_dir() && os_access(path, false),
                write: Some(path.is_dir() && os_access(path, true)),
            });
        }
    }
    checks
}

fn title_word(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Browse
// ---------------------------------------------------------------------------

async fn browse(
    State(state): State<AppState>,
    Query(query): Query<BrowseQuery>,
) -> Json<BrowseResponse> {
    let roots = state.db.list_input_roots();
    let active_root = match query.root_id {
        Some(id) => state.db.get_input_root(id),
        None => roots.first().cloned(),
    };
    let root_infos: Vec<BrowseRootInfo> = roots
        .iter()
        .map(|r| BrowseRootInfo {
            id: r.id,
            path: r.path.clone(),
        })
        .collect();
    let active_info = active_root.as_ref().map(|r| BrowseRootInfo {
        id: r.id,
        path: r.path.clone(),
    });

    let mut entries = Vec::new();
    let mut error = None;
    let mut parent_path = String::new();

    if let Some(root) = active_root {
        let root_path = PathBuf::from(&root.path);
        match list_directory(&root_path, &query.path) {
            Ok(listed) => {
                // Map each entry to the set of source files it covers.
                let mut entry_sources: HashMap<String, Vec<PathBuf>> = HashMap::new();
                let mut all_sources: Vec<PathBuf> = Vec::new();
                for entry in &listed {
                    let sources = if entry.is_dir {
                        let mut present = expand_source_files(
                            &root_path,
                            std::slice::from_ref(&entry.relative_path),
                        )
                        .unwrap_or_default();
                        // Files imported from this folder and then moved out of
                        // the input tree no longer exist to be listed, but the
                        // folder should still reflect that they were imported.
                        let present_set: std::collections::HashSet<&PathBuf> = present.iter().collect();
                        let moved: Vec<PathBuf> = state
                            .db
                            .latest_imports_under_prefix(&entry.absolute_path)
                            .into_iter()
                            .map(|row| PathBuf::from(row.source_path))
                            .filter(|p| !present_set.contains(p))
                            .collect();
                        present.extend(moved);
                        present
                    } else {
                        vec![canonical_or_normalized(&entry.absolute_path)]
                    };
                    for s in &sources {
                        all_sources.push(s.clone());
                    }
                    entry_sources.insert(entry.relative_path.clone(), sources);
                }
                let imports = state.db.latest_imports_for_sources(&all_sources);
                let overrides = state.db.source_status_overrides(&all_sources);

                for entry in listed {
                    let sources = entry_sources.get(&entry.relative_path).cloned().unwrap_or_default();
                    let status = compute_browse_status(&sources, &imports, &overrides);
                    entries.push(BrowseEntry {
                        name: entry.name,
                        relative_path: entry.relative_path,
                        is_dir: entry.is_dir,
                        is_video: entry.is_video,
                        is_hardlink: entry.is_hardlink,
                        size: entry.size,
                        size_human: human_file_size(entry.size),
                        status: status.status,
                        status_key: status.status_key,
                        manual_status: status.manual_status,
                        latest_import_result: status.latest_import_result,
                        source_count: status.source_count,
                    });
                }
                parent_path = parent_relative(&query.path);
            }
            Err(e) => error = Some(e.to_string()),
        }
    }

    Json(BrowseResponse {
        roots: root_infos,
        active_root: active_info,
        current_path: query.path,
        parent_path,
        entries,
        error,
    })
}

struct BrowseStatus {
    status: Option<String>,
    status_key: String,
    manual_status: String,
    latest_import_result: Option<String>,
    source_count: usize,
}

fn source_status_for_path(
    source: &Path,
    imports: &HashMap<String, ImportRow>,
    overrides: &HashMap<String, crate::db::SourceStatusOverride>,
) -> (Option<String>, String, Option<String>) {
    let key = canonical_or_normalized(source).to_string_lossy().to_string();
    let override_status = overrides.get(&key);
    let latest = imports.get(&key);
    let status = if let Some(o) = override_status {
        if o.status == "none" {
            None
        } else {
            Some(o.status.clone())
        }
    } else {
        latest.map(|l| l.result.clone())
    };
    let manual_status = override_status
        .map(|o| o.status.clone())
        .unwrap_or_else(|| "auto".to_string());
    let latest_result = latest.map(|l| l.result.clone());
    (status, manual_status, latest_result)
}

fn compute_browse_status(
    sources: &[PathBuf],
    imports: &HashMap<String, ImportRow>,
    overrides: &HashMap<String, crate::db::SourceStatusOverride>,
) -> BrowseStatus {
    let states: Vec<(Option<String>, String, Option<String>)> = sources
        .iter()
        .map(|s| source_status_for_path(s, imports, overrides))
        .collect();
    let status_keys: std::collections::HashSet<String> = states
        .iter()
        .map(|(status, _, _)| status.clone().unwrap_or_else(|| "none".to_string()))
        .collect();

    let (status, status_key) = if states.is_empty() || status_keys == std::iter::once("none".to_string()).collect() {
        (None, "none".to_string())
    } else if status_keys.len() == 1 {
        let key = status_keys.into_iter().next().unwrap();
        if key == "none" {
            (None, "none".to_string())
        } else {
            (Some(key.clone()), key)
        }
    } else {
        (Some("mixed".to_string()), "mixed".to_string())
    };

    let latest_import_result = if states.len() == 1 {
        states[0].2.clone()
    } else {
        None
    };
    let manual_statuses: std::collections::HashSet<String> =
        states.iter().map(|(_, m, _)| m.clone()).collect();
    let manual_status = if states.len() == 1 {
        states[0].1.clone()
    } else if manual_statuses.len() > 1 {
        String::new()
    } else {
        states.first().map(|(_, m, _)| m.clone()).unwrap_or_default()
    };

    BrowseStatus {
        status,
        status_key,
        manual_status,
        latest_import_result,
        source_count: states.len(),
    }
}

// ---------------------------------------------------------------------------
// Match
// ---------------------------------------------------------------------------

async fn match_route(
    State(state): State<AppState>,
    Json(payload): Json<MatchPayload>,
) -> AppResult<Json<MatchResponse>> {
    let root = state
        .db
        .get_input_root(payload.root_id)
        .ok_or_else(|| AppError::not_found("Input root not found"))?;
    if !is_valid_media_type(&payload.media_type) {
        return Err(AppError::bad_request("Invalid media type"));
    }
    let media_type = payload.media_type.clone();
    let root_path = PathBuf::from(&root.path);
    let selected = payload.selected.clone();
    let groups = tokio::task::spawn_blocking(move || expand_grouped(&root_path, &selected))
        .await
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| AppError::bad_request(e.to_string()))?;

    let mut search_cache: HashMap<String, Vec<ShowCandidate>> = HashMap::new();
    let mut episode_cache: HashMap<String, Vec<EpisodeCandidate>> = HashMap::new();
    let mut out_groups: Vec<MatchGroup> = Vec::new();

    for group in groups {
        if media_type == "film" {
            // Films carry their identity in the filename; one group per file.
            for gfile in group.files {
                let mut parsed = parse_film_filename(&gfile.path);
                parsed.quality = probe_quality_fallback(&gfile.path, &parsed.quality).await;
                let enriched = enrich_film(&state, &parsed, &mut search_cache).await;
                out_groups.push(MatchGroup {
                    group_key: gfile.path.to_string_lossy().to_string(),
                    group_name: parsed.source_name.clone(),
                    show_title: enriched.show_title,
                    show_year: enriched.show_year,
                    provider: enriched.provider,
                    provider_show_id: enriched.provider_show_id,
                    candidates: enriched.candidates,
                    metadata_error: enriched.metadata_error,
                    episodes: vec![MatchEpisode {
                        source_path: gfile.path.to_string_lossy().to_string(),
                        source_name: parsed.source_name.clone(),
                        season_number: 0,
                        episode_number: 0,
                        episode_title: "Film".to_string(),
                        quality: parsed.quality.clone(),
                        parsed,
                    }],
                });
            }
        } else {
            out_groups.push(
                enrich_group(
                    &state,
                    &media_type,
                    group,
                    &mut search_cache,
                    &mut episode_cache,
                )
                .await,
            );
        }
    }

    Ok(Json(MatchResponse {
        media_type: media_type.clone(),
        output_root: state
            .output_root_for(&media_type)
            .map(|p| p.to_string_lossy().to_string()),
        groups: out_groups,
    }))
}

/// Resolve quality via ffprobe for a single file when the filename lacked a token.
async fn probe_quality_fallback(path: &Path, current: &str) -> String {
    if current != "Unknown" {
        return current.to_string();
    }
    let probe_path = path.to_path_buf();
    match tokio::task::spawn_blocking(move || crate::ffprobe::probe_quality(&probe_path)).await {
        Ok(Some(quality)) => quality,
        _ => current.to_string(),
    }
}

#[derive(Clone)]
struct Enriched {
    show_title: String,
    show_year: Option<i64>,
    provider: String,
    provider_show_id: String,
    candidates: Vec<ShowCandidate>,
    metadata_error: Option<String>,
}

/// Match a whole folder to one show: one provider search + one episode fetch,
/// then map every file in the group to a season/episode/title.
async fn enrich_group(
    state: &AppState,
    media_type: &str,
    group: crate::filesystem::FileGroup,
    search_cache: &mut HashMap<String, Vec<ShowCandidate>>,
    episode_cache: &mut HashMap<String, Vec<EpisodeCandidate>>,
) -> MatchGroup {
    let (folder_title, folder_year) = crate::parser::show_title_from_folder(&group.group_name);

    // One search for the whole folder.
    let search_result = match search_cache.get(&folder_title) {
        Some(cached) => Ok(cached.clone()),
        None => match state.providers.search(media_type, &folder_title).await {
            Ok(found) => {
                search_cache.insert(folder_title.clone(), found.clone());
                Ok(found)
            }
            Err(err) => Err(err.user_message()),
        },
    };

    let (candidates, metadata_error) = match search_result {
        Ok(found) => (found, None),
        Err(message) => (Vec::new(), Some(message)),
    };
    let selected = candidates.first().cloned();

    // One episode fetch for the selected show.
    let episodes = match &selected {
        Some(candidate) => {
            let key = format!("{}:{}", media_type, candidate.provider_id);
            match episode_cache.get(&key) {
                Some(cached) => cached.clone(),
                None => match state.providers.episodes(media_type, &candidate.provider_id).await {
                    Ok(found) => {
                        episode_cache.insert(key, found.clone());
                        found
                    }
                    Err(_) => Vec::new(),
                },
            }
        }
        None => Vec::new(),
    };

    let show_title = selected
        .as_ref()
        .map(|c| c.title.clone())
        .unwrap_or_else(|| folder_title.clone());
    let show_year = selected.as_ref().and_then(|c| c.year).or(folder_year);

    let mut out_episodes = Vec::new();
    for gfile in &group.files {
        let segments: Vec<&str> = gfile
            .relative_segments
            .iter()
            .map(|s| s.as_str())
            .collect();
        let season_hint = crate::parser::season_from_segments(&segments);
        let mut parsed =
            crate::parser::parse_folder_episode(&gfile.path, &show_title, season_hint);
        parsed.quality = probe_quality_fallback(&gfile.path, &parsed.quality).await;

        let episode_title = episodes
            .iter()
            .find(|e| e.season == parsed.season && e.episode == parsed.episode)
            .or_else(|| {
                if media_type == "anime" {
                    episodes.iter().find(|e| e.episode == parsed.episode)
                } else {
                    None
                }
            })
            .map(|e| e.title.clone())
            .unwrap_or_else(|| parsed.episode_title.clone());

        out_episodes.push(MatchEpisode {
            source_path: gfile.path.to_string_lossy().to_string(),
            source_name: parsed.source_name.clone(),
            season_number: parsed.season,
            episode_number: parsed.episode,
            episode_title,
            quality: parsed.quality.clone(),
            parsed,
        });
    }

    MatchGroup {
        group_key: group.group_key,
        group_name: group.group_name,
        show_title,
        show_year,
        provider: selected.as_ref().map(|c| c.provider.clone()).unwrap_or_default(),
        provider_show_id: selected.map(|c| c.provider_id).unwrap_or_default(),
        candidates,
        metadata_error,
        episodes: out_episodes,
    }
}

async fn enrich_film(
    state: &AppState,
    parsed: &ParsedMedia,
    search_cache: &mut HashMap<String, Vec<ShowCandidate>>,
) -> Enriched {
    let fallback = Enriched {
        show_title: parsed.title.clone(),
        show_year: parsed.year,
        provider: String::new(),
        provider_show_id: String::new(),
        candidates: Vec::new(),
        metadata_error: None,
    };

    let candidates = match search_cache.get(&parsed.title) {
        Some(cached) => cached.clone(),
        None => match state.providers.search("film", &parsed.title).await {
            Ok(found) => {
                search_cache.insert(parsed.title.clone(), found.clone());
                found
            }
            Err(err) => {
                return Enriched {
                    metadata_error: Some(err.user_message()),
                    ..fallback
                };
            }
        },
    };

    if candidates.is_empty() {
        return fallback;
    }
    let selected = candidates
        .iter()
        .find(|c| parsed.year.is_some() && c.year == parsed.year)
        .cloned()
        .unwrap_or_else(|| candidates[0].clone());

    Enriched {
        show_title: selected.title,
        show_year: selected.year.or(parsed.year),
        provider: selected.provider,
        provider_show_id: selected.provider_id,
        candidates,
        metadata_error: None,
    }
}

// ---------------------------------------------------------------------------
// Preview & imports
// ---------------------------------------------------------------------------

async fn preview(
    State(state): State<AppState>,
    Json(batch): Json<ImportBatch>,
) -> AppResult<Json<Value>> {
    let requests = build_import_requests(&state, &batch)?;
    let mut results = Vec::new();
    for request in requests {
        let result = preview_import(request);
        results.push(json!({
            "source_path": result.source_path,
            "final_path": result.final_path,
            "result": result.result,
            "error": result.error,
        }));
    }
    Ok(Json(json!({ "results": results })))
}

async fn start_import_job(
    State(state): State<AppState>,
    Json(batch): Json<ImportBatch>,
) -> AppResult<Json<Value>> {
    let requests = build_import_requests(&state, &batch)?;
    let rate = state.copy_rate_limit_mbps();
    let job = state.jobs.start(requests, state.db.clone(), rate);
    Ok(Json(snapshot_json(&job)))
}

fn snapshot_json(job: &crate::jobs::Job) -> Value {
    serde_json::to_value(job.snapshot()).unwrap_or_else(|_| json!({}))
}

async fn list_import_jobs(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let jobs: Vec<Value> = state
        .jobs
        .list()
        .into_iter()
        .map(|job| snapshot_json(&job))
        .collect();
    Ok(Json(json!({ "jobs": jobs })))
}

async fn clear_import_jobs(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let cleared = state.jobs.clear_completed();
    let jobs: Vec<Value> = state
        .jobs
        .list()
        .into_iter()
        .map(|job| snapshot_json(&job))
        .collect();
    Ok(Json(json!({ "cleared": cleared, "jobs": jobs })))
}

async fn get_import_job(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> AppResult<Json<Value>> {
    let job = state.jobs.get(&id).ok_or_else(|| AppError::not_found("Import job not found"))?;
    Ok(Json(snapshot_json(&job)))
}

async fn cancel_import_item(
    State(state): State<AppState>,
    AxumPath((id, index)): AxumPath<(String, usize)>,
) -> AppResult<Json<Value>> {
    let job = state.jobs.get(&id).ok_or_else(|| AppError::not_found("Import job not found"))?;
    job.request_cancel_item(index);
    Ok(Json(snapshot_json(&job)))
}

async fn cancel_import_job(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> AppResult<Json<Value>> {
    let job = state.jobs.get(&id).ok_or_else(|| AppError::not_found("Import job not found"))?;
    job.request_cancel();
    Ok(Json(snapshot_json(&job)))
}

async fn import_job_results(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> AppResult<Json<Value>> {
    let job = state.jobs.get(&id).ok_or_else(|| AppError::not_found("Import job not found"))?;
    if !job.is_finished() {
        return Err(AppError::conflict("Import job is not done"));
    }
    let results: Vec<Value> = job
        .results()
        .into_iter()
        .map(|r| {
            json!({
                "source_path": r.source_path,
                "final_path": r.final_path,
                "result": r.result,
                "error": r.error,
            })
        })
        .collect();
    Ok(Json(json!({ "results": results })))
}

fn build_import_requests(state: &AppState, batch: &ImportBatch) -> AppResult<Vec<ImportRequest>> {
    if !is_valid_media_type(&batch.media_type) {
        return Err(AppError::bad_request("Invalid media type"));
    }
    if !is_valid_action(&batch.action) {
        return Err(AppError::bad_request("Invalid action"));
    }
    if !is_valid_conflict_policy(&batch.conflict_policy) {
        return Err(AppError::bad_request("Invalid conflict policy"));
    }
    let output_root = state
        .output_root_for(&batch.media_type)
        .ok_or_else(|| AppError::bad_request(format!("No {} output root configured", batch.media_type)))?;

    let input_roots: Vec<PathBuf> = state.db.list_input_roots().into_iter().map(|r| PathBuf::from(r.path)).collect();
    let mut requests = Vec::new();
    for item in &batch.items {
        let source = canonical_or_normalized(&PathBuf::from(&item.source_path));
        assert_source_allowed(&source, &input_roots)?;
        requests.push(ImportRequest {
            source_path: source,
            output_root: output_root.clone(),
            media_type: batch.media_type.clone(),
            show_title: item.show_title.clone(),
            show_year: item.show_year,
            season_number: item.season_number,
            episode_number: item.episode_number,
            episode_title: item.episode_title.clone(),
            quality: item.quality.clone(),
            action: batch.action.clone(),
            conflict_policy: batch.conflict_policy.clone(),
            provider: item.provider.clone().filter(|s| !s.is_empty()),
            provider_show_id: item.provider_show_id.clone().filter(|s| !s.is_empty()),
        });
    }
    Ok(requests)
}

fn assert_source_allowed(source: &Path, input_roots: &[PathBuf]) -> AppResult<()> {
    if input_roots.iter().any(|root| is_relative_to(source, root)) {
        Ok(())
    } else {
        Err(AppError::bad_request(format!(
            "Source is outside configured input roots: {}",
            source.display()
        )))
    }
}

// ---------------------------------------------------------------------------
// Library & history
// ---------------------------------------------------------------------------

async fn library(State(state): State<AppState>) -> Json<Value> {
    let files: Vec<Value> = state
        .db
        .list_library_files()
        .into_iter()
        .map(|f| {
            json!({
                "media_type": f.media_type,
                "output_path": f.output_path,
                "present": f.present,
                "size": f.size,
                "size_human": human_file_size(f.size),
            })
        })
        .collect();
    let roots: HashMap<String, String> = state
        .output_roots()
        .into_iter()
        .map(|(k, v)| (k, v.to_string_lossy().to_string()))
        .collect();
    Json(json!({ "files": files, "roots": roots }))
}

async fn rescan_library(State(state): State<AppState>) -> Json<Value> {
    let roots = state.output_roots();
    let counts = tokio::task::spawn_blocking(move || {
        let db = state.db.clone();
        crate::library::rescan_outputs(&db, &roots)
    })
    .await
    .unwrap_or_default();
    Json(json!({ "counts": counts }))
}

async fn history(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "imports": state.db.list_imports(100) }))
}

// ---------------------------------------------------------------------------
// Search / episodes
// ---------------------------------------------------------------------------

async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> AppResult<Json<Value>> {
    if !is_valid_media_type(&query.media_type) {
        return Err(AppError::bad_request("Invalid media type"));
    }
    let q = query.q.trim();
    if q.is_empty() {
        return Ok(Json(json!({ "results": [] })));
    }
    let results = state
        .providers
        .search(&query.media_type, q)
        .await
        .map_err(|e| AppError::new(StatusCode::BAD_GATEWAY, e.user_message()))?;
    Ok(Json(json!({ "results": results })))
}

async fn episodes(
    State(state): State<AppState>,
    Query(query): Query<EpisodesQuery>,
) -> AppResult<Json<Value>> {
    if !is_valid_media_type(&query.media_type) {
        return Err(AppError::bad_request("Invalid media type"));
    }
    let results = state
        .providers
        .episodes(&query.media_type, &query.provider_show_id)
        .await
        .map_err(|e| AppError::new(StatusCode::BAD_GATEWAY, e.user_message()))?;
    Ok(Json(json!({ "results": results })))
}

// ---------------------------------------------------------------------------
// Source status
// ---------------------------------------------------------------------------

async fn source_status(
    State(state): State<AppState>,
    Json(payload): Json<SourceStatusPayload>,
) -> AppResult<Json<Value>> {
    let sources = status_update_sources(&state, &payload)?;
    if sources.is_empty() {
        return Err(AppError::bad_request("No files selected"));
    }
    if payload.status == "auto" {
        state.db.set_source_status_overrides(&sources, None);
        return Ok(Json(json!({ "status": "auto", "updated": sources.len() })));
    }
    if !crate::state::SOURCE_STATUSES.contains(&payload.status.as_str()) {
        return Err(AppError::bad_request("Invalid status"));
    }
    state.db.set_source_status_overrides(&sources, Some(&payload.status));
    Ok(Json(json!({ "status": payload.status, "updated": sources.len() })))
}

fn status_update_sources(state: &AppState, payload: &SourceStatusPayload) -> AppResult<Vec<PathBuf>> {
    let input_roots: Vec<PathBuf> = state.db.list_input_roots().into_iter().map(|r| PathBuf::from(r.path)).collect();
    if let Some(source_path) = &payload.source_path {
        let source = canonical_or_normalized(&PathBuf::from(source_path));
        assert_source_allowed(&source, &input_roots)?;
        return Ok(vec![source]);
    }
    let root_id = payload.root_id.ok_or_else(|| AppError::bad_request("Input root is required"))?;
    let root = state.db.get_input_root(root_id).ok_or_else(|| AppError::not_found("Input root not found"))?;
    expand_source_files(&PathBuf::from(&root.path), &payload.selected)
        .map_err(|e| AppError::bad_request(e.to_string()))
}

// ---------------------------------------------------------------------------
// Folder picker
// ---------------------------------------------------------------------------

async fn folders(Query(query): Query<FoldersQuery>) -> AppResult<Json<FoldersResponse>> {
    let current = resolve_picker_path(&query.path)?;
    let mut children: Vec<PathBuf> = std::fs::read_dir(&current)
        .map_err(|e| AppError::bad_request(e.to_string()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    children.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default());

    let folders: Vec<FolderEntry> = children
        .into_iter()
        .filter(|p| std::fs::metadata(p).is_ok())
        .map(|p| FolderEntry {
            name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            path: p.to_string_lossy().to_string(),
        })
        .collect();

    let parent = current.parent().filter(|p| *p != current).map(|p| p.to_string_lossy().to_string());
    let roots: Vec<String> = PICKER_ROOTS
        .iter()
        .filter(|root| Path::new(root).exists())
        .map(|root| root.to_string())
        .collect();

    Ok(Json(FoldersResponse {
        path: current.to_string_lossy().to_string(),
        parent,
        folders,
        roots,
    }))
}

fn resolve_picker_path(value: &str) -> AppResult<PathBuf> {
    let path = canonical_or_normalized(&expand_user(if value.is_empty() { "/" } else { value }));
    if !path.exists() {
        return Err(AppError::not_found(format!("Folder does not exist: {}", path.display())));
    }
    if !path.is_dir() {
        return Err(AppError::bad_request(format!("Path is not a folder: {}", path.display())));
    }
    Ok(path)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn expand_user(value: &str) -> PathBuf {
    let trimmed = value.trim();
    if let Some(rest) = trimmed.strip_prefix("~") {
        if let Some(home) = std::env::var_os("HOME") {
            let mut path = PathBuf::from(home);
            let rest = rest.trim_start_matches('/');
            if !rest.is_empty() {
                path.push(rest);
            }
            return path;
        }
    }
    PathBuf::from(trimmed)
}

fn normalize_path(value: &str) -> String {
    canonical_or_normalized(&expand_user(value)).to_string_lossy().to_string()
}

fn normalize_optional(value: &str) -> String {
    if value.trim().is_empty() {
        String::new()
    } else {
        normalize_path(value)
    }
}

fn normalize_copy_rate_limit(value: &str) -> String {
    let cleaned = value.replace(',', ".");
    let cleaned = cleaned.trim();
    let mut limit: f64 = cleaned.parse().unwrap_or(15.0);
    limit = limit.clamp(0.0, 1000.0);
    if limit.fract() == 0.0 {
        format!("{}", limit as i64)
    } else {
        let formatted = format!("{limit:.2}");
        formatted.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn parent_relative(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    match Path::new(path).parent() {
        Some(parent) => {
            let s = parent.to_string_lossy().to_string();
            if s == "." {
                String::new()
            } else {
                s
            }
        }
        None => String::new(),
    }
}

#[cfg(unix)]
fn os_access(path: &Path, write: bool) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = match CString::new(path.as_os_str().as_bytes()) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mode = if write { libc::W_OK } else { libc::R_OK };
    unsafe { libc::access(c.as_ptr(), mode) == 0 }
}

#[cfg(not(unix))]
fn os_access(_path: &Path, _write: bool) -> bool {
    false
}
