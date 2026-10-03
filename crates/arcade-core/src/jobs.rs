//! Bounded, cancellable background execution for registered tools.
//!
//! Requests and results stay in memory. SQLite stores only job identifiers,
//! tool IDs, lifecycle status, coarse progress, and timestamps.

use arcade_contract::{ResultStatus, ToolRequest, ToolResult};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use thiserror::Error;
use uuid::Uuid;

use crate::{Arcade, storage::StoredJob};

const DEFAULT_WORKERS: usize = 2;
const DEFAULT_QUEUE_CAPACITY: usize = 32;
const MAX_WORKERS: usize = 16;
const MAX_QUEUE_CAPACITY: usize = 1024;
const RETAINED_TERMINAL_JOBS: usize = 100;
const PROGRESS_EVENT_INTERVAL: Duration = Duration::from_millis(200);
const PROGRESS_EVENT_DELTA: f64 = 0.02;

pub type JobUpdateHandler = Arc<dyn Fn(JobSnapshot) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl JobStatus {
    fn as_storage_value(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    fn from_storage_value(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "cancelling" => Self::Cancelling,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            // Unknown/legacy states are never resumed blindly.
            _ => Self::Interrupted,
        }
    }

    fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }
}

/// A UI-safe view of a job. `message` and `result` are deliberately memory-only;
/// persisted records never contain document text, file paths, or output values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub id: String,
    pub tool_id: String,
    pub status: JobStatus,
    pub progress: Option<f64>,
    pub message: Option<String>,
    pub result: Option<ToolResult>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<StoredJob> for JobSnapshot {
    fn from(job: StoredJob) -> Self {
        let status = JobStatus::from_storage_value(&job.status);
        Self {
            id: job.id,
            tool_id: job.tool_id,
            status,
            progress: job.progress,
            message: (status == JobStatus::Interrupted)
                .then(|| "This job was interrupted when Arcade Box last closed.".to_owned()),
            result: None,
            created_at: job.created_at,
            updated_at: job.updated_at,
        }
    }
}

#[derive(Debug, Error)]
pub enum JobError {
    #[error("The background job queue is full")]
    QueueFull,
    #[error("Job {0} was not found")]
    NotFound(String),
    #[error("Job {id} cannot be cancelled because it is already {status:?}")]
    NotCancellable { id: String, status: JobStatus },
    #[error("The job manager is shutting down")]
    ShuttingDown,
    #[error("Worker count and queue capacity must be within supported bounds")]
    InvalidLimits,
    #[error("Could not start job worker: {0}")]
    WorkerSpawn(#[source] std::io::Error),
    #[error("Job metadata storage failed: {0}")]
    Storage(#[from] rusqlite::Error),
}

struct JobEntry {
    snapshot: JobSnapshot,
    request: Option<ToolRequest>,
    cancellation: Arc<AtomicBool>,
    last_progress_event: Instant,
}

#[derive(Default)]
struct QueueState {
    jobs: HashMap<String, JobEntry>,
    queued: VecDeque<String>,
    stopping: bool,
}

struct Shared {
    runtime: Arc<Arcade>,
    on_update: JobUpdateHandler,
    state: Mutex<QueueState>,
    wake: Condvar,
    queue_capacity: usize,
}

/// A process-local bounded worker pool for background tools.
pub struct JobManager {
    shared: Arc<Shared>,
    // Dropping JoinHandles detaches workers. Drop signals cancellation and
    // shutdown; it never blocks the app's exit path waiting for a large job.
    _workers: Vec<JoinHandle<()>>,
}

/// Handle for a long-lived operation whose resources are owned outside the
/// core worker pool (for example a desktop portal recording session). It uses
/// the same persisted lifecycle, cancellation signal and update stream as a
/// normal tool job.
#[derive(Clone)]
pub struct ExternalJobHandle {
    shared: Weak<Shared>,
    id: String,
    cancellation: Arc<AtomicBool>,
}

impl ExternalJobHandle {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn is_cancellation_requested(&self) -> bool {
        self.cancellation.load(Ordering::Acquire)
    }

    pub fn request_cancellation(&self) {
        self.cancellation.store(true, Ordering::Release);
    }

    /// Publish rate-limited progress and a user-facing status message.
    pub fn update(
        &self,
        progress: Option<f64>,
        message: Option<String>,
    ) -> Result<JobSnapshot, JobError> {
        let shared = self.shared.upgrade().ok_or(JobError::ShuttingDown)?;
        let snapshot = {
            let mut state = lock_state(&shared);
            let entry = state
                .jobs
                .get_mut(&self.id)
                .ok_or_else(|| JobError::NotFound(self.id.clone()))?;
            if entry.snapshot.status != JobStatus::Running {
                return Err(JobError::NotCancellable {
                    id: self.id.clone(),
                    status: entry.snapshot.status,
                });
            }
            let progress = progress
                .filter(|value| value.is_finite())
                .map(|value| value.clamp(0.0, 1.0));
            let changed = progress.is_some_and(|value| {
                entry
                    .snapshot
                    .progress
                    .is_none_or(|previous| (previous - value).abs() >= PROGRESS_EVENT_DELTA)
            }) || message
                .as_ref()
                .is_some_and(|value| entry.snapshot.message.as_ref() != Some(value));
            if let Some(progress) = progress {
                entry.snapshot.progress = Some(progress);
            }
            if message.is_some() {
                entry.snapshot.message = message;
            }
            if !changed && entry.last_progress_event.elapsed() < PROGRESS_EVENT_INTERVAL {
                return Ok(entry.snapshot.clone());
            }
            entry.last_progress_event = Instant::now();
            persist_snapshot(&shared, entry);
            entry.snapshot.clone()
        };
        emit_update(&shared, snapshot.clone());
        Ok(snapshot)
    }

    /// Complete the external operation. Results remain in memory only, like
    /// results from normal tool jobs; persisted history contains metadata only.
    pub fn finish(&self, execution: Result<ToolResult, String>) -> Result<JobSnapshot, JobError> {
        let shared = self.shared.upgrade().ok_or(JobError::ShuttingDown)?;
        let snapshot = {
            let mut state = lock_state(&shared);
            let entry = state
                .jobs
                .get_mut(&self.id)
                .ok_or_else(|| JobError::NotFound(self.id.clone()))?;
            if entry.snapshot.status.is_terminal() {
                return Ok(entry.snapshot.clone());
            }
            match execution {
                Ok(result) if result.status == ResultStatus::Success => {
                    entry.snapshot.status = JobStatus::Succeeded;
                    entry.snapshot.progress = Some(1.0);
                    entry.snapshot.message = result.message.clone();
                    entry.snapshot.result = Some(result);
                }
                Ok(result) => {
                    entry.snapshot.status = JobStatus::Failed;
                    entry.snapshot.message = result.message.clone();
                    entry.snapshot.result = Some(result);
                }
                Err(_) if self.is_cancellation_requested() => {
                    entry.snapshot.status = JobStatus::Cancelled;
                    entry.snapshot.progress = None;
                    entry.snapshot.message = Some("Job cancelled".into());
                    entry.snapshot.result = None;
                }
                Err(error) => {
                    entry.snapshot.status = JobStatus::Failed;
                    entry.snapshot.progress = None;
                    entry.snapshot.message = Some(error);
                    entry.snapshot.result = None;
                }
            }
            entry.request = None;
            persist_snapshot(&shared, entry);
            let snapshot = entry.snapshot.clone();
            trim_memory_history(&mut state);
            let _ = shared
                .runtime
                .storage()
                .prune_terminal_jobs(RETAINED_TERMINAL_JOBS);
            snapshot
        };
        emit_update(&shared, snapshot.clone());
        Ok(snapshot)
    }
}

impl JobManager {
    pub fn new(runtime: Arc<Arcade>, on_update: JobUpdateHandler) -> Result<Self, JobError> {
        Self::with_limits(runtime, DEFAULT_WORKERS, DEFAULT_QUEUE_CAPACITY, on_update)
    }

    /// Create a manager with explicit limits. This also supports focused tests
    /// that need deterministic queue saturation.
    pub fn with_limits(
        runtime: Arc<Arcade>,
        worker_count: usize,
        queue_capacity: usize,
        on_update: JobUpdateHandler,
    ) -> Result<Self, JobError> {
        if worker_count == 0
            || worker_count > MAX_WORKERS
            || queue_capacity == 0
            || queue_capacity > MAX_QUEUE_CAPACITY
        {
            return Err(JobError::InvalidLimits);
        }

        runtime.storage().recover_interrupted_jobs()?;
        runtime
            .storage()
            .prune_terminal_jobs(RETAINED_TERMINAL_JOBS)?;
        let restored = runtime.storage().recent_jobs(RETAINED_TERMINAL_JOBS)?;
        let mut state = QueueState::default();
        for record in restored {
            let snapshot = JobSnapshot::from(record);
            state.jobs.insert(
                snapshot.id.clone(),
                JobEntry {
                    snapshot,
                    request: None,
                    cancellation: Arc::new(AtomicBool::new(false)),
                    last_progress_event: Instant::now(),
                },
            );
        }

        let shared = Arc::new(Shared {
            runtime,
            on_update,
            state: Mutex::new(state),
            wake: Condvar::new(),
            queue_capacity,
        });
        let mut workers = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let worker_shared = shared.clone();
            match thread::Builder::new()
                .name(format!("arcade-job-{index}"))
                .spawn(move || worker_loop(worker_shared))
            {
                Ok(worker) => workers.push(worker),
                Err(error) => {
                    {
                        let mut state = lock_state(&shared);
                        state.stopping = true;
                    }
                    shared.wake.notify_all();
                    drop(workers);
                    return Err(JobError::WorkerSpawn(error));
                }
            }
        }
        Ok(Self {
            shared,
            _workers: workers,
        })
    }

    /// Enqueue a tool request. The request itself is never persisted.
    pub fn submit(&self, request: ToolRequest) -> Result<JobSnapshot, JobError> {
        let id = Uuid::new_v4().to_string();
        let tool_id = request.tool_id.clone();
        let snapshot = {
            let mut state = lock_state(&self.shared);
            if state.stopping {
                return Err(JobError::ShuttingDown);
            }
            if state.queued.len() >= self.shared.queue_capacity {
                return Err(JobError::QueueFull);
            }

            let record = self.shared.runtime.storage().create_job(&id, &tool_id)?;
            let snapshot = JobSnapshot::from(record);
            state.jobs.insert(
                id.clone(),
                JobEntry {
                    snapshot: snapshot.clone(),
                    request: Some(request),
                    cancellation: Arc::new(AtomicBool::new(false)),
                    last_progress_event: Instant::now(),
                },
            );
            state.queued.push_back(id);
            trim_memory_history(&mut state);
            // Pruning is metadata-only and never removes pending/running rows.
            let _ = self
                .shared
                .runtime
                .storage()
                .prune_terminal_jobs(RETAINED_TERMINAL_JOBS);
            snapshot
        };

        emit_update(&self.shared, snapshot.clone());
        self.shared.wake.notify_one();
        Ok(snapshot)
    }

    /// Register an already-running, externally-managed operation in the same
    /// job history and event stream as regular tool execution.
    pub fn begin_external(
        &self,
        tool_id: &str,
        message: impl Into<String>,
    ) -> Result<(JobSnapshot, ExternalJobHandle), JobError> {
        let id = Uuid::new_v4().to_string();
        let cancellation = Arc::new(AtomicBool::new(false));
        let snapshot = {
            let mut state = lock_state(&self.shared);
            if state.stopping {
                return Err(JobError::ShuttingDown);
            }
            let active_count = state
                .jobs
                .values()
                .filter(|entry| !entry.snapshot.status.is_terminal())
                .count();
            if active_count >= self.shared.queue_capacity {
                return Err(JobError::QueueFull);
            }
            let record = self.shared.runtime.storage().create_job(&id, tool_id)?;
            let mut snapshot = JobSnapshot::from(record);
            snapshot.status = JobStatus::Running;
            snapshot.message = Some(message.into());
            let mut entry = JobEntry {
                snapshot,
                request: None,
                cancellation: cancellation.clone(),
                last_progress_event: Instant::now(),
            };
            persist_snapshot(&self.shared, &mut entry);
            let snapshot = entry.snapshot.clone();
            state.jobs.insert(id.clone(), entry);
            trim_memory_history(&mut state);
            snapshot
        };
        emit_update(&self.shared, snapshot.clone());
        Ok((
            snapshot,
            ExternalJobHandle {
                shared: Arc::downgrade(&self.shared),
                id,
                cancellation,
            },
        ))
    }

    /// Request cancellation. Queued jobs are cancelled immediately; running
    /// jobs receive a cooperative cancellation signal.
    pub fn cancel(&self, id: &str) -> Result<JobSnapshot, JobError> {
        let (snapshot, should_emit) = {
            let mut state = lock_state(&self.shared);
            if state.stopping {
                return Err(JobError::ShuttingDown);
            }
            let status = state
                .jobs
                .get(id)
                .ok_or_else(|| JobError::NotFound(id.to_owned()))?
                .snapshot
                .status;

            match status {
                JobStatus::Queued => {
                    state.queued.retain(|queued_id| queued_id != id);
                    let entry = state.jobs.get_mut(id).expect("job was just found");
                    entry.cancellation.store(true, Ordering::Release);
                    entry.request = None;
                    entry.snapshot.status = JobStatus::Cancelled;
                    entry.snapshot.progress = None;
                    entry.snapshot.message = Some("Job cancelled before it started".into());
                }
                JobStatus::Running => {
                    let entry = state.jobs.get_mut(id).expect("job was just found");
                    entry.cancellation.store(true, Ordering::Release);
                    entry.snapshot.status = JobStatus::Cancelling;
                    entry.snapshot.message = Some("Cancellation requested".into());
                }
                JobStatus::Cancelling => {
                    return Ok(state
                        .jobs
                        .get(id)
                        .expect("job was just found")
                        .snapshot
                        .clone());
                }
                status => {
                    return Err(JobError::NotCancellable {
                        id: id.to_owned(),
                        status,
                    });
                }
            }

            let entry = state.jobs.get_mut(id).expect("job was just found");
            persist_snapshot(&self.shared, entry);
            let snapshot = entry.snapshot.clone();
            trim_memory_history(&mut state);
            let _ = self
                .shared
                .runtime
                .storage()
                .prune_terminal_jobs(RETAINED_TERMINAL_JOBS);
            (snapshot, true)
        };

        if should_emit {
            emit_update(&self.shared, snapshot.clone());
        }
        Ok(snapshot)
    }

    /// Return active work and the most recent terminal job metadata.
    pub fn list(&self) -> Vec<JobSnapshot> {
        let state = lock_state(&self.shared);
        let mut jobs: Vec<_> = state
            .jobs
            .values()
            .map(|job| job.snapshot.clone())
            .collect();
        jobs.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| right.id.cmp(&left.id))
        });
        jobs
    }
}

impl Drop for JobManager {
    fn drop(&mut self) {
        let snapshots = {
            let mut state = lock_state(&self.shared);
            state.stopping = true;
            let queued_ids: Vec<_> = state.queued.drain(..).collect();
            let mut snapshots = Vec::with_capacity(queued_ids.len());
            for id in queued_ids {
                if let Some(entry) = state.jobs.get_mut(&id)
                    && entry.snapshot.status == JobStatus::Queued
                {
                    entry.request = None;
                    entry.snapshot.status = JobStatus::Interrupted;
                    entry.snapshot.message =
                        Some("Application closed before this job started".into());
                    persist_snapshot(&self.shared, entry);
                    snapshots.push(entry.snapshot.clone());
                }
            }
            for entry in state.jobs.values_mut() {
                if entry.snapshot.status == JobStatus::Running {
                    entry.cancellation.store(true, Ordering::Release);
                    entry.snapshot.status = JobStatus::Cancelling;
                    entry.snapshot.message = Some("Application is shutting down".into());
                    persist_snapshot(&self.shared, entry);
                }
            }
            snapshots
        };
        for snapshot in snapshots {
            emit_update(&self.shared, snapshot);
        }
        self.shared.wake.notify_all();
    }
}

fn worker_loop(shared: Arc<Shared>) {
    loop {
        let task = {
            let mut state = lock_state(&shared);
            while state.queued.is_empty() && !state.stopping {
                state = shared
                    .wake
                    .wait(state)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            if state.stopping {
                return;
            }
            let Some(id) = state.queued.pop_front() else {
                continue;
            };
            let Some(entry) = state.jobs.get_mut(&id) else {
                continue;
            };
            if entry.snapshot.status != JobStatus::Queued {
                continue;
            }

            entry.snapshot.status = JobStatus::Running;
            entry.snapshot.message = None;
            persist_snapshot(&shared, entry);
            let snapshot = entry.snapshot.clone();
            let request = entry.request.take();
            let cancellation = entry.cancellation.clone();
            request.map(|request| (id, request, cancellation, snapshot))
        };

        let Some((id, request, cancellation, snapshot)) = task else {
            continue;
        };
        emit_update(&shared, snapshot);

        let progress_shared = shared.clone();
        let progress_id = id.clone();
        let progress: Arc<dyn Fn(f64) + Send + Sync> =
            Arc::new(move |value| publish_progress(&progress_shared, &progress_id, value));
        // A cancellation can arrive while the Running event is being delivered.
        // Avoid starting work at all when it was requested before execution.
        if cancellation.load(Ordering::Acquire) {
            finish_job(
                &shared,
                &id,
                &cancellation,
                Ok(Err(crate::CoreError::Execution(
                    "cancelled before execution".into(),
                ))),
            );
            continue;
        }
        let execution = catch_unwind(AssertUnwindSafe(|| {
            shared
                .runtime
                .run_tool_with_progress(request, &cancellation, progress)
        }));
        finish_job(&shared, &id, &cancellation, execution);
    }
}

fn publish_progress(shared: &Shared, id: &str, progress: f64) {
    if !progress.is_finite() {
        return;
    }
    let progress = progress.clamp(0.0, 1.0);
    let snapshot = {
        let mut state = lock_state(shared);
        let Some(entry) = state.jobs.get_mut(id) else {
            return;
        };
        if entry.snapshot.status != JobStatus::Running {
            return;
        }
        let should_emit = entry
            .snapshot
            .progress
            .is_none_or(|previous| (progress - previous).abs() >= PROGRESS_EVENT_DELTA)
            || entry.last_progress_event.elapsed() >= PROGRESS_EVENT_INTERVAL;
        entry.snapshot.progress = Some(progress);
        if !should_emit {
            return;
        }
        entry.last_progress_event = Instant::now();
        persist_snapshot(shared, entry);
        entry.snapshot.clone()
    };
    emit_update(shared, snapshot);
}

fn finish_job(
    shared: &Shared,
    id: &str,
    cancellation: &AtomicBool,
    execution: std::thread::Result<Result<ToolResult, crate::CoreError>>,
) {
    let mut state = lock_state(shared);
    let Some(entry) = state.jobs.get_mut(id) else {
        return;
    };

    match execution {
        // If cancellation raced after the tool had already produced a
        // successful result, keep that result visible instead of discarding it.
        Ok(Ok(result)) if result.status == ResultStatus::Success => {
            entry.snapshot.status = JobStatus::Succeeded;
            entry.snapshot.message = result.message.clone();
            entry.snapshot.progress = Some(1.0);
            entry.snapshot.result = Some(result);
        }
        Ok(Ok(_result)) if cancellation.load(Ordering::Acquire) => {
            entry.snapshot.status = JobStatus::Cancelled;
            entry.snapshot.message = Some("Job cancelled".into());
            entry.snapshot.result = None;
        }
        Ok(Err(_)) | Err(_) if cancellation.load(Ordering::Acquire) => {
            entry.snapshot.status = JobStatus::Cancelled;
            entry.snapshot.message = Some("Job cancelled".into());
            entry.snapshot.result = None;
        }
        Ok(Ok(result)) => {
            entry.snapshot.status = JobStatus::Failed;
            entry.snapshot.message = result.message.clone();
            entry.snapshot.result = Some(result);
        }
        Ok(Err(error)) => {
            entry.snapshot.status = JobStatus::Failed;
            entry.snapshot.message = Some(error.to_string());
            entry.snapshot.result = None;
        }
        Err(_) => {
            entry.snapshot.status = JobStatus::Failed;
            entry.snapshot.message = Some("The job worker stopped unexpectedly".into());
            entry.snapshot.result = None;
        }
    }

    persist_snapshot(shared, entry);
    let snapshot = entry.snapshot.clone();
    trim_memory_history(&mut state);
    let _ = shared
        .runtime
        .storage()
        .prune_terminal_jobs(RETAINED_TERMINAL_JOBS);
    drop(state);
    emit_update(shared, snapshot);
}

fn persist_snapshot(shared: &Shared, entry: &mut JobEntry) {
    if let Ok(record) = shared.runtime.storage().update_job(
        &entry.snapshot.id,
        entry.snapshot.status.as_storage_value(),
        entry.snapshot.progress,
    ) {
        entry.snapshot.created_at = record.created_at;
        entry.snapshot.updated_at = record.updated_at;
    }
}

fn trim_memory_history(state: &mut QueueState) {
    let mut terminal: Vec<_> = state
        .jobs
        .iter()
        .filter(|(_, entry)| entry.snapshot.status.is_terminal())
        .map(|(id, entry)| (id.clone(), entry.snapshot.updated_at.clone()))
        .collect();
    terminal.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| right.0.cmp(&left.0)));
    for (id, _) in terminal.into_iter().skip(RETAINED_TERMINAL_JOBS) {
        state.jobs.remove(&id);
    }
}

fn emit_update(shared: &Shared, snapshot: JobSnapshot) {
    let _ = catch_unwind(AssertUnwindSafe(|| (shared.on_update)(snapshot)));
}

fn lock_state(shared: &Shared) -> std::sync::MutexGuard<'_, QueueState> {
    shared
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_contract::{ToolValue, ValueKind};
    use std::{
        sync::{
            atomic::{AtomicBool as TestAtomicBool, Ordering as TestOrdering},
            mpsc,
        },
        time::Instant,
    };

    fn request() -> ToolRequest {
        ToolRequest {
            tool_id: "arcade.text.case".into(),
            inputs: vec![ToolValue {
                kind: ValueKind::Text,
                value: "hello world".into(),
                mime: "text/plain".into(),
            }],
            options: serde_json::json!({"mode":"upper"}),
        }
    }

    fn wait_for(
        manager: &JobManager,
        id: &str,
        predicate: impl Fn(JobStatus) -> bool,
    ) -> JobSnapshot {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(job) = manager.list().into_iter().find(|job| job.id == id)
                && predicate(job.status)
            {
                return job;
            }
            assert!(Instant::now() < deadline, "timed out waiting for job state");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn executes_tool_and_retains_result_in_memory() {
        let runtime = Arc::new(Arcade::in_memory().unwrap());
        let manager = JobManager::new(runtime, Arc::new(|_| {})).unwrap();
        let queued = manager.submit(request()).unwrap();
        let finished = wait_for(&manager, &queued.id, |status| status.is_terminal());
        assert_eq!(finished.status, JobStatus::Succeeded);
        let result = finished.result.expect("completed result retained");
        assert_eq!(result.outputs[0].value, "HELLO WORLD");
        let persisted = manager
            .shared
            .runtime
            .storage()
            .recent_jobs(10)
            .unwrap()
            .into_iter()
            .find(|job| job.id == queued.id)
            .unwrap();
        assert_eq!(persisted.status, "succeeded");
        assert!(
            !serde_json::to_value(persisted)
                .unwrap()
                .to_string()
                .contains("HELLO WORLD")
        );
    }

    #[test]
    fn bounded_queue_rejects_excess_work_and_cancels_queued_job() {
        let runtime = Arc::new(Arcade::in_memory().unwrap());
        let (running_tx, running_rx) = mpsc::channel();
        let released = Arc::new((Mutex::new(false), Condvar::new()));
        let callback_release = released.clone();
        let first_running = Arc::new(TestAtomicBool::new(false));
        let callback_first_running = first_running.clone();
        let callback = Arc::new(move |snapshot: JobSnapshot| {
            if snapshot.status == JobStatus::Running
                && !callback_first_running.swap(true, TestOrdering::AcqRel)
            {
                let _ = running_tx.send(snapshot.id);
                let (lock, wake) = &*callback_release;
                let mut ready = lock.lock().unwrap();
                while !*ready {
                    ready = wake.wait(ready).unwrap();
                }
            }
        });
        let manager = JobManager::with_limits(runtime, 1, 1, callback).unwrap();
        let first = manager.submit(request()).unwrap();
        assert_eq!(
            running_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            first.id
        );
        let second = manager.submit(request()).unwrap();
        assert!(matches!(
            manager.submit(request()),
            Err(JobError::QueueFull)
        ));

        let cancelled = manager.cancel(&second.id).unwrap();
        assert_eq!(cancelled.status, JobStatus::Cancelled);
        {
            let (lock, wake) = &*released;
            *lock.lock().unwrap() = true;
            wake.notify_all();
        }
        assert_eq!(
            wait_for(&manager, &first.id, |status| status.is_terminal()).status,
            JobStatus::Succeeded
        );
    }

    #[test]
    fn cancellation_of_running_job_is_cooperative_and_visible() {
        let runtime = Arc::new(Arcade::in_memory().unwrap());
        let (running_tx, running_rx) = mpsc::channel();
        let released = Arc::new((Mutex::new(false), Condvar::new()));
        let callback_release = released.clone();
        let first_running = Arc::new(TestAtomicBool::new(false));
        let callback_first_running = first_running.clone();
        let callback = Arc::new(move |snapshot: JobSnapshot| {
            if snapshot.status == JobStatus::Running
                && !callback_first_running.swap(true, TestOrdering::AcqRel)
            {
                let _ = running_tx.send(snapshot.id);
                let (lock, wake) = &*callback_release;
                let mut ready = lock.lock().unwrap();
                while !*ready {
                    ready = wake.wait(ready).unwrap();
                }
            }
        });
        let manager = JobManager::with_limits(runtime, 1, 1, callback).unwrap();
        let job = manager.submit(request()).unwrap();
        assert_eq!(
            running_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            job.id
        );
        assert_eq!(
            manager.cancel(&job.id).unwrap().status,
            JobStatus::Cancelling
        );
        {
            let (lock, wake) = &*released;
            *lock.lock().unwrap() = true;
            wake.notify_all();
        }
        assert_eq!(
            wait_for(&manager, &job.id, |status| status.is_terminal()).status,
            JobStatus::Cancelled
        );
    }

    #[test]
    fn external_session_uses_the_shared_job_lifecycle_and_cancellation_signal() {
        let runtime = Arc::new(Arcade::in_memory().unwrap());
        let manager = JobManager::new(runtime, Arc::new(|_| {})).unwrap();
        let (started, external) = manager
            .begin_external("arcade.screen.recorder", "Waiting for screen selection")
            .unwrap();
        assert_eq!(started.status, JobStatus::Running);
        assert!(started.result.is_none());
        external
            .update(None, Some("Recording selected display".into()))
            .unwrap();
        assert_eq!(
            manager.cancel(&started.id).unwrap().status,
            JobStatus::Cancelling
        );
        assert!(external.is_cancellation_requested());
        let completed = external
            .finish(Err("Screen recording cancelled".into()))
            .unwrap();
        assert_eq!(completed.status, JobStatus::Cancelled);
        assert!(completed.result.is_none());
    }
}
