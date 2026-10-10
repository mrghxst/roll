//! Background jobs. Jobs are rows in the `jobs` table, run one at a time in
//! priority order on a worker thread, and are written so that running them
//! again continues where they stopped. Running jobs check in regularly to
//! report progress, pause and stop.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::{Condvar, Mutex};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::{now_ms, Db};
use crate::model::JobInfo;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobSpec {
    Scan { root_id: i64 },
    Thumbs,
    Hash,
}

impl JobSpec {
    pub fn kind(&self) -> &'static str {
        match self {
            JobSpec::Scan { .. } => "scan",
            JobSpec::Thumbs => "thumbs",
            JobSpec::Hash => "hash",
        }
    }
    pub fn priority(&self) -> i64 {
        match self {
            JobSpec::Scan { .. } => 10,
            JobSpec::Thumbs => 20,
            JobSpec::Hash => 30,
        }
    }
    pub fn label(&self) -> String {
        match self {
            JobSpec::Scan { .. } => "Indexing the library".into(),
            JobSpec::Thumbs => "Making thumbnails".into(),
            JobSpec::Hash => "Checking files for duplicates".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Run,
    Pause,
    Cancel,
}

struct Live {
    started: Instant,
    done: usize,
    total: usize,
    control: Control,
    cost: f64,
}

struct Shared {
    live: Mutex<HashMap<i64, Live>>,
    /// Wakes paused jobs (used with `live`).
    resumed: Condvar,
    queue_lock: Mutex<()>,
    /// Wakes the worker when jobs are queued (used with `queue_lock`).
    queued: Condvar,
}

pub type Executor = Arc<dyn Fn(&JobSpec, &JobCtx) -> anyhow::Result<()> + Send + Sync>;
pub type Notify = Arc<dyn Fn(Vec<JobInfo>) + Send + Sync>;

#[derive(Clone)]
pub struct Jobs {
    db: Arc<Db>,
    shared: Arc<Shared>,
    notify: Notify,
}

/// Handed to a running job.
pub struct JobCtx {
    pub id: i64,
    jobs: Jobs,
}

impl JobCtx {
    /// Report progress. Returns false when the job should stop; blocks
    /// while it is paused.
    pub fn progress(&self, done: usize, total: usize) -> bool {
        {
            let mut live = self.jobs.shared.live.lock();
            if let Some(l) = live.get_mut(&self.id) {
                l.done = done;
                l.total = total;
            }
        }
        self.jobs.emit();
        self.checkpoint()
    }

    pub fn checkpoint(&self) -> bool {
        let mut live = self.jobs.shared.live.lock();
        loop {
            match live.get(&self.id).map(|l| l.control) {
                Some(Control::Pause) => {
                    self.jobs.shared.resumed.wait(&mut live);
                }
                Some(Control::Cancel) => return false,
                _ => return true,
            }
        }
    }

    pub fn add_cost(&self, usd: f64) {
        if let Some(l) = self.jobs.shared.live.lock().get_mut(&self.id) {
            l.cost += usd;
        }
    }
}

impl Jobs {
    pub fn new(db: Arc<Db>, notify: Notify) -> Self {
        // Jobs that were running when the app stopped continue.
        let _ = db.write().execute("UPDATE jobs SET state = 'queued' WHERE state = 'running'", []);
        Self { db, shared: Arc::new(Shared { live: Mutex::new(HashMap::new()), resumed: Condvar::new(), queue_lock: Mutex::new(()), queued: Condvar::new() }), notify }
    }

    /// Queue a job unless the same one is already waiting or running.
    pub fn enqueue(&self, spec: JobSpec) -> anyhow::Result<i64> {
        let params_json = serde_json::to_string(&spec)?;
        let id = {
            let c = self.db.write();
            let existing: Option<i64> = c
                .query_row("SELECT id FROM jobs WHERE params = ?1 AND state IN ('queued', 'running', 'paused')", [&params_json], |r| r.get(0))
                .ok();
            match existing {
                Some(id) => id,
                None => {
                    c.execute(
                        "INSERT INTO jobs (kind, label, state, priority, params, created_at) VALUES (?1, ?2, 'queued', ?3, ?4, ?5)",
                        params![spec.kind(), spec.label(), spec.priority(), params_json, now_ms()],
                    )?;
                    c.last_insert_rowid()
                }
            }
        };
        self.wake_worker();
        self.emit();
        Ok(id)
    }

    pub fn pause(&self, id: i64) {
        if let Some(l) = self.shared.live.lock().get_mut(&id) {
            l.control = Control::Pause;
        }
        let _ = self.db.write().execute("UPDATE jobs SET state = 'paused' WHERE id = ?1 AND state IN ('queued', 'running')", [id]);
        self.emit();
    }

    pub fn resume(&self, id: i64) {
        let running = {
            let mut live = self.shared.live.lock();
            match live.get_mut(&id) {
                Some(l) => {
                    l.control = Control::Run;
                    true
                }
                None => false,
            }
        };
        let state = if running { "running" } else { "queued" };
        let _ = self.db.write().execute("UPDATE jobs SET state = ?2 WHERE id = ?1 AND state = 'paused'", params![id, state]);
        self.shared.resumed.notify_all();
        self.wake_worker();
        self.emit();
    }

    pub fn cancel(&self, id: i64) {
        if let Some(l) = self.shared.live.lock().get_mut(&id) {
            l.control = Control::Cancel;
        }
        let _ = self.db.write().execute("UPDATE jobs SET state = 'cancelled', finished_at = ?2 WHERE id = ?1 AND state IN ('queued', 'paused')", params![id, now_ms()]);
        self.shared.resumed.notify_all();
        self.emit();
    }

    fn wake_worker(&self) {
        let _g = self.shared.queue_lock.lock();
        self.shared.queued.notify_all();
    }

    /// Recent and active jobs for the Activity panel.
    pub fn list(&self) -> Vec<JobInfo> {
        let live = self.shared.live.lock();
        let c = self.db.read();
        let Ok(mut st) = c.prepare(
            "SELECT id, kind, label, state, done, total, cost_usd, error FROM jobs
             WHERE state IN ('queued', 'running', 'paused') OR finished_at > ?1 ORDER BY id DESC LIMIT 30",
        ) else {
            return Vec::new();
        };
        let since = now_ms() - 6 * 3600 * 1000;
        st.query_map([since], |r| {
            let id: i64 = r.get(0)?;
            let mut info = JobInfo {
                id: id as i32,
                kind: r.get(1)?,
                label: r.get(2)?,
                state: r.get(3)?,
                done: r.get(4)?,
                total: r.get(5)?,
                rate: 0.0,
                eta_secs: None,
                cost_usd: r.get(6)?,
                error: r.get(7)?,
            };
            if let Some(l) = live.get(&id) {
                info.done = l.done as i32;
                info.total = l.total as i32;
                info.cost_usd += l.cost;
                let secs = l.started.elapsed().as_secs_f32().max(0.001);
                info.rate = l.done as f32 / secs;
                if info.rate > 0.0 && l.total > l.done {
                    info.eta_secs = Some(((l.total - l.done) as f32 / info.rate) as i32);
                }
            }
            Ok(info)
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    fn emit(&self) {
        (self.notify)(self.list());
    }

    fn next(&self) -> Option<(i64, JobSpec)> {
        let c = self.db.write();
        let row: Option<(i64, String)> =
            c.query_row("SELECT id, params FROM jobs WHERE state = 'queued' ORDER BY priority, id LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?))).ok();
        let (id, p) = row?;
        let spec: JobSpec = match serde_json::from_str(&p) {
            Ok(s) => s,
            Err(e) => {
                let _ = c.execute("UPDATE jobs SET state = 'failed', error = ?2 WHERE id = ?1", params![id, e.to_string()]);
                return None;
            }
        };
        let _ = c.execute("UPDATE jobs SET state = 'running', started_at = coalesce(started_at, ?2) WHERE id = ?1", params![id, now_ms()]);
        Some((id, spec))
    }

    /// Start the worker thread.
    pub fn start(&self, exec: Executor) {
        let jobs = self.clone();
        std::thread::Builder::new()
            .name("roll-jobs".into())
            .spawn(move || loop {
                // Holding the queue lock while looking means a job queued in
                // between cannot be missed: its wake-up waits for our wait.
                let (id, spec) = {
                    let mut g = jobs.shared.queue_lock.lock();
                    match jobs.next() {
                        Some(next) => next,
                        None => {
                            jobs.shared.queued.wait_for(&mut g, std::time::Duration::from_secs(30));
                            continue;
                        }
                    }
                };
                jobs.shared.live.lock().insert(id, Live { started: Instant::now(), done: 0, total: 0, control: Control::Run, cost: 0.0 });
                jobs.emit();
                let ctx = JobCtx { id, jobs: jobs.clone() };
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| exec(&spec, &ctx)));
                let live = jobs.shared.live.lock().remove(&id);
                let (state, error) = match (res, live.as_ref().map(|l| l.control)) {
                    (_, Some(Control::Cancel)) => ("cancelled", None),
                    (Ok(Ok(())), _) => ("done", None),
                    (Ok(Err(e)), _) => ("failed", Some(e.to_string())),
                    (Err(_), _) => ("failed", Some("The job stopped unexpectedly.".to_string())),
                };
                let (done, total, cost) = live.map(|l| (l.done as i64, l.total as i64, l.cost)).unwrap_or_default();
                let _ = jobs.db.write().execute(
                    "UPDATE jobs SET state = ?2, error = ?3, done = ?4, total = ?5, cost_usd = cost_usd + ?6, finished_at = ?7 WHERE id = ?1",
                    params![id, state, error, done, total, cost, now_ms()],
                );
                jobs.emit();
            })
            .expect("spawn job thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn runs_in_priority_order_and_dedups() {
        let db = Arc::new(Db::memory().unwrap());
        let jobs = Jobs::new(db.clone(), Arc::new(|_| {}));
        jobs.enqueue(JobSpec::Hash).unwrap();
        let a = jobs.enqueue(JobSpec::Thumbs).unwrap();
        assert_eq!(jobs.enqueue(JobSpec::Thumbs).unwrap(), a);
        jobs.enqueue(JobSpec::Scan { root_id: 1 }).unwrap();
        let order = Arc::new(Mutex::new(Vec::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let (o, n) = (order.clone(), count.clone());
        jobs.start(Arc::new(move |spec, ctx| {
            o.lock().push(spec.kind());
            assert!(ctx.progress(1, 1));
            n.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        for _ in 0..200 {
            if count.load(Ordering::SeqCst) == 3 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(*order.lock(), vec!["scan", "thumbs", "hash"]);
        assert!(jobs.list().iter().all(|j| j.state == "done"));
    }
}
