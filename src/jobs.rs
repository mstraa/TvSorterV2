use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use uuid::Uuid;

use crate::db::Database;
use crate::importer::{execute_import, import_request_units, result_to_record, ImportRequest, ImportResult};

/// One file within an import job. `status` is one of:
/// queued | running | imported | skipped | preview | failed | cancelled.
#[derive(Clone, Serialize)]
pub struct JobItem {
    pub index: usize,
    pub name: String,
    pub destination: String,
    pub status: String,
    pub bytes: u64,
    pub total: u64,
    pub error: Option<String>,
}

#[derive(Default)]
struct JobState {
    completed_units: u64,
    state: String,
    cancel_all: bool,
    cancel_items: HashSet<usize>,
    items: Vec<JobItem>,
    results: Vec<ImportResult>,
    error: Option<String>,
}

pub struct Job {
    pub id: String,
    pub seq: u64,
    pub label: String,
    pub total_items: usize,
    pub total_units: u64,
    state: Arc<Mutex<JobState>>,
}

#[derive(Serialize)]
pub struct JobSnapshot {
    pub id: String,
    pub seq: u64,
    pub label: String,
    pub state: String,
    pub percent: u32,
    pub completed: u64,
    pub total: u64,
    pub total_items: usize,
    pub completed_items: usize,
    pub failed_items: usize,
    pub cancelled_items: usize,
    pub active: bool,
    pub error: Option<String>,
    pub items: Vec<JobItem>,
}

impl Job {
    fn lock(&self) -> std::sync::MutexGuard<'_, JobState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> JobSnapshot {
        let s = self.lock();
        let percent = if self.total_units == 0 {
            100
        } else {
            ((s.completed_units as f64 / self.total_units as f64) * 100.0).min(100.0) as u32
        };
        let mut completed_items = 0;
        let mut failed_items = 0;
        let mut cancelled_items = 0;
        for item in &s.items {
            match item.status.as_str() {
                "imported" | "skipped" | "preview" => completed_items += 1,
                "failed" => failed_items += 1,
                "cancelled" => cancelled_items += 1,
                _ => {}
            }
        }
        JobSnapshot {
            id: self.id.clone(),
            seq: self.seq,
            label: self.label.clone(),
            state: s.state.clone(),
            percent,
            completed: s.completed_units,
            total: self.total_units,
            total_items: self.total_items,
            completed_items,
            failed_items,
            cancelled_items,
            active: s.state == "running",
            error: s.error.clone(),
            items: s.items.clone(),
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.lock().state.as_str(), "done" | "cancelled" | "failed")
    }

    pub fn results(&self) -> Vec<ImportResult> {
        self.lock().results.clone()
    }

    /// Cancel the whole job: the running file is aborted and every queued file
    /// is skipped.
    pub fn request_cancel(&self) {
        let mut s = self.lock();
        if s.state == "running" {
            s.cancel_all = true;
        }
    }

    /// Cancel a single file by its 1-based index. A queued file is skipped; the
    /// file currently copying is aborted while the rest of the job continues.
    pub fn request_cancel_item(&self, index: usize) {
        let mut s = self.lock();
        if s.state != "running" {
            return;
        }
        // No-op if the file already reached a terminal state.
        let still_active = s
            .items
            .get(index.wrapping_sub(1))
            .map(|it| it.status == "queued" || it.status == "running")
            .unwrap_or(false);
        if still_active {
            s.cancel_items.insert(index);
        }
    }
}

#[derive(Clone, Default)]
pub struct JobManager {
    jobs: Arc<Mutex<Vec<Arc<Job>>>>,
    seq: Arc<AtomicU64>,
}

impl JobManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|j| j.id == id)
            .cloned()
    }

    /// All jobs, newest first.
    pub fn list(&self) -> Vec<Arc<Job>> {
        let mut jobs: Vec<Arc<Job>> =
            self.jobs.lock().unwrap_or_else(|e| e.into_inner()).clone();
        jobs.sort_by(|a, b| b.seq.cmp(&a.seq));
        jobs
    }

    pub fn start(
        &self,
        requests: Vec<ImportRequest>,
        db: Database,
        copy_rate_limit_mbps: Option<f64>,
    ) -> Arc<Job> {
        let total_units: u64 = requests.iter().map(import_request_units).sum();
        let items: Vec<JobItem> = requests
            .iter()
            .enumerate()
            .map(|(i, request)| JobItem {
                index: i + 1,
                name: request
                    .source_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                destination: String::new(),
                status: "queued".to_string(),
                bytes: 0,
                total: import_request_units(request),
                error: None,
            })
            .collect();
        let media_type = requests
            .first()
            .map(|r| r.media_type.clone())
            .unwrap_or_default();
        let label = format!(
            "{} {} · {} item{}",
            requests.len(),
            if media_type.is_empty() { "media" } else { &media_type },
            requests.len(),
            if requests.len() == 1 { "" } else { "s" }
        );

        let job = Arc::new(Job {
            id: Uuid::new_v4().simple().to_string(),
            seq: self.seq.fetch_add(1, Ordering::SeqCst),
            label,
            total_items: requests.len(),
            total_units,
            state: Arc::new(Mutex::new(JobState {
                state: "running".to_string(),
                items,
                ..Default::default()
            })),
        });
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(job.clone());

        let worker_state = job.state.clone();
        let worker_total = total_units;
        tokio::task::spawn_blocking(move || {
            run_job(requests, worker_state, worker_total, db, copy_rate_limit_mbps);
        });

        job
    }
}

fn lock(state: &Arc<Mutex<JobState>>) -> std::sync::MutexGuard<'_, JobState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// Mark every still-queued file as cancelled (used when the whole job stops).
fn cancel_remaining(s: &mut JobState) {
    for item in s.items.iter_mut() {
        if item.status == "queued" || item.status == "running" {
            item.status = "cancelled".to_string();
        }
    }
}

fn run_job(
    requests: Vec<ImportRequest>,
    state: Arc<Mutex<JobState>>,
    total_units: u64,
    db: Database,
    copy_rate_limit_mbps: Option<f64>,
) {
    for (zero_index, request) in requests.into_iter().enumerate() {
        let index = zero_index + 1;
        let item_units = import_request_units(&request);

        // Pre-flight checks under the lock.
        {
            let mut s = lock(&state);
            if s.cancel_all {
                cancel_remaining(&mut s);
                s.state = "cancelled".to_string();
                return;
            }
            // This file was cancelled while still queued: skip it, keep going.
            if s.cancel_items.contains(&index) {
                if let Some(item) = s.items.get_mut(zero_index) {
                    item.status = "cancelled".to_string();
                }
                s.completed_units += item_units;
                continue;
            }
            if let Some(item) = s.items.get_mut(zero_index) {
                item.status = "running".to_string();
                item.bytes = 0;
                item.total = item_units;
            }
        }

        let item_start = lock(&state).completed_units;

        let progress_state = state.clone();
        let progress = move |copied: u64, total: u64| {
            let mut s = progress_state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(item) = s.items.get_mut(zero_index) {
                item.bytes = copied;
                item.total = total.max(item_units);
            }
            s.completed_units = if total == 0 {
                item_start + item_units
            } else {
                item_start + ((copied as f64 / total as f64) * item_units as f64) as u64
            };
        };

        let cancel_state = state.clone();
        let cancel = move || {
            let s = cancel_state.lock().unwrap_or_else(|e| e.into_inner());
            s.cancel_all || s.cancel_items.contains(&index)
        };

        let result = execute_import(request, Some(&progress), copy_rate_limit_mbps, Some(&cancel));
        db.insert_import(&result_to_record(&result));

        let mut s = lock(&state);
        let status = result.result.clone();
        let final_path = result.final_path.clone();
        let error = result.error.clone();
        s.results.push(result);
        if let Some(item) = s.items.get_mut(zero_index) {
            item.status = status;
            item.destination = final_path;
            item.error = error;
            item.bytes = item.total;
        }
        s.completed_units = item_start + item_units;

        // A whole-job cancel may have arrived while this file was copying.
        if s.cancel_all {
            cancel_remaining(&mut s);
            s.state = "cancelled".to_string();
            return;
        }
    }

    let mut s = lock(&state);
    if s.state != "cancelled" {
        s.completed_units = total_units;
        s.state = "done".to_string();
    }
}
