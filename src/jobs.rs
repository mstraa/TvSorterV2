use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use uuid::Uuid;

use crate::db::Database;
use crate::importer::{execute_import, import_request_units, result_to_record, ImportRequest, ImportResult};

#[derive(Default)]
struct JobState {
    completed_units: u64,
    completed_items: usize,
    current_item_index: usize,
    current_item: String,
    current_action: String,
    current_item_bytes: u64,
    current_item_total: u64,
    state: String,
    cancel_requested: bool,
    results: Vec<ImportResult>,
    error: Option<String>,
}

pub struct Job {
    pub id: String,
    pub total_items: usize,
    pub total_units: u64,
    state: Arc<Mutex<JobState>>,
}

#[derive(Serialize)]
pub struct JobSnapshot {
    pub id: String,
    pub state: String,
    pub percent: u32,
    pub current_item: String,
    pub current_action: String,
    pub current_item_index: usize,
    pub current_item_bytes: u64,
    pub current_item_total: u64,
    pub current_item_percent: u32,
    pub completed: u64,
    pub total: u64,
    pub completed_items: usize,
    pub total_items: usize,
    pub cancel_requested: bool,
    pub error: Option<String>,
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
        let item_percent = if s.current_item_total == 0 {
            0
        } else {
            ((s.current_item_bytes as f64 / s.current_item_total as f64) * 100.0).min(100.0) as u32
        };
        JobSnapshot {
            id: self.id.clone(),
            state: s.state.clone(),
            percent,
            current_item: s.current_item.clone(),
            current_action: s.current_action.clone(),
            current_item_index: s.current_item_index,
            current_item_bytes: s.current_item_bytes,
            current_item_total: s.current_item_total,
            current_item_percent: item_percent,
            completed: s.completed_units,
            total: self.total_units,
            completed_items: s.completed_items,
            total_items: self.total_items,
            cancel_requested: s.cancel_requested,
            error: s.error.clone(),
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.lock().state.as_str(), "done" | "cancelled" | "failed")
    }

    pub fn results(&self) -> Vec<ImportResult> {
        self.lock().results.clone()
    }

    pub fn request_cancel(&self) {
        let mut s = self.lock();
        if s.state == "running" {
            s.cancel_requested = true;
        }
    }
}

#[derive(Clone, Default)]
pub struct JobManager {
    jobs: Arc<Mutex<HashMap<String, Arc<Job>>>>,
}

impl JobManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
    }

    pub fn start(
        &self,
        requests: Vec<ImportRequest>,
        db: Database,
        copy_rate_limit_mbps: Option<f64>,
    ) -> Arc<Job> {
        let total_units: u64 = requests.iter().map(import_request_units).sum();
        let job = Arc::new(Job {
            id: Uuid::new_v4().simple().to_string(),
            total_items: requests.len(),
            total_units,
            state: Arc::new(Mutex::new(JobState {
                state: "running".to_string(),
                ..Default::default()
            })),
        });
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(job.id.clone(), job.clone());

        let worker_state = job.state.clone();
        let worker_total = total_units;
        tokio::task::spawn_blocking(move || {
            run_job(requests, worker_state, worker_total, db, copy_rate_limit_mbps);
        });

        job
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
        let item_start;
        {
            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
            if s.cancel_requested {
                s.state = "cancelled".to_string();
                break;
            }
            item_start = s.completed_units;
            s.current_item_index = index;
            s.current_item = request
                .source_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            s.current_action = request.action.clone();
            s.current_item_bytes = 0;
            s.current_item_total = item_units;
        }

        let progress_state = state.clone();
        let progress = move |copied: u64, total: u64| {
            let mut s = progress_state.lock().unwrap_or_else(|e| e.into_inner());
            s.current_item_bytes = copied;
            s.current_item_total = total;
            if total == 0 {
                s.completed_units = item_start + item_units;
            } else {
                s.completed_units =
                    item_start + ((copied as f64 / total as f64) * item_units as f64) as u64;
            }
        };

        let cancel_state = state.clone();
        let cancel = move || cancel_state.lock().unwrap_or_else(|e| e.into_inner()).cancel_requested;

        let result = execute_import(request, Some(&progress), copy_rate_limit_mbps, Some(&cancel));
        db.insert_import(&result_to_record(&result));

        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
        let cancelled = result.result == "cancelled";
        s.results.push(result);
        if cancelled {
            s.cancel_requested = true;
            s.state = "cancelled".to_string();
            break;
        }
        s.completed_items = index;
        s.current_item_bytes = s.current_item_total;
        s.completed_units = item_start + item_units;
    }

    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
    s.current_item.clear();
    s.current_action.clear();
    s.current_item_index = 0;
    s.current_item_bytes = 0;
    s.current_item_total = 0;
    if s.state != "cancelled" {
        s.completed_units = total_units;
        s.state = "done".to_string();
    }
}
