//! In-memory background job registry for `code_execute`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::ExecOutcome;

static JOB_SEQ: AtomicU64 = AtomicU64::new(1);

/// Opaque handle identifying a background sandbox job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub String);

/// Current lifecycle state of a background sandbox job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Finished(ExecOutcome),
    NotFound,
}

/// Thread-safe registry tracking active and completed background sandbox runs.
#[derive(Clone, Default)]
pub struct JobRegistry {
    jobs: Arc<Mutex<HashMap<String, JobStatus>>>,
}

impl JobRegistry {
    /// Creates an empty job registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates a new `JobId` initialized in `Running` state.
    pub fn allocate(&self) -> JobId {
        let seq = JOB_SEQ.fetch_add(1, Ordering::Relaxed);
        let id = format!("job_{seq}");
        self.jobs
            .lock()
            .expect("job registry mutex poisoned")
            .insert(id.clone(), JobStatus::Running);
        JobId(id)
    }

    /// Marks `id` as finished with `outcome`.
    pub fn finish(&self, id: &JobId, outcome: ExecOutcome) {
        self.jobs
            .lock()
            .expect("job registry mutex poisoned")
            .insert(id.0.clone(), JobStatus::Finished(outcome));
    }

    /// Queries the current status of `id`.
    pub fn status(&self, id: &JobId) -> JobStatus {
        self.jobs
            .lock()
            .expect("job registry mutex poisoned")
            .get(&id.0)
            .cloned()
            .unwrap_or(JobStatus::NotFound)
    }
}

/// Returns the current `JobStatus` for `job_id`.
pub fn job_status(registry: &JobRegistry, job_id: &JobId) -> JobStatus {
    registry.status(job_id)
}
