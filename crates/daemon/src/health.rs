//! Task heartbeats, the supervisor that restarts failed tasks, and the systemd
//! watchdog that only pets when every task is alive.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

struct Entry {
    last_beat: Instant,
    max_silence: Duration,
    restarts: u32,
    last_ok: Option<DateTime<Utc>>,
    last_error: Option<String>,
}

#[derive(Default)]
struct Inner {
    tasks: BTreeMap<String, Entry>,
    stalled: HashSet<String>,
}

#[derive(Clone, Default)]
pub struct Health(Arc<Mutex<Inner>>);

#[derive(Debug, Serialize)]
pub struct TaskStatus {
    pub name: String,
    pub healthy: bool,
    pub silent_secs: u64,
    pub max_silence_secs: u64,
    pub restarts: u32,
    pub last_ok: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

impl Health {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn register(&self, name: &str, max_silence: Duration) {
        let mut g = self.lock();
        let e = g.tasks.entry(name.to_string()).or_insert(Entry {
            last_beat: Instant::now(),
            max_silence,
            restarts: 0,
            last_ok: None,
            last_error: None,
        });
        e.max_silence = max_silence;
        e.last_beat = Instant::now();
    }

    pub fn beat(&self, name: &str) {
        if let Some(e) = self.lock().tasks.get_mut(name) {
            e.last_beat = Instant::now();
        }
    }

    /// Records that the task's real work succeeded (shown as "last success").
    pub fn ok(&self, name: &str) {
        if let Some(e) = self.lock().tasks.get_mut(name) {
            e.last_beat = Instant::now();
            e.last_ok = Some(Utc::now());
            e.last_error = None;
        }
    }

    pub fn error(&self, name: &str, err: impl std::fmt::Display) {
        if let Some(e) = self.lock().tasks.get_mut(name) {
            e.last_error = Some(err.to_string());
        }
    }

    fn restarted(&self, name: &str, err: String) {
        if let Some(e) = self.lock().tasks.get_mut(name) {
            e.restarts += 1;
            e.last_error = Some(err);
        }
    }

    pub fn statuses(&self) -> Vec<TaskStatus> {
        let now = Instant::now();
        self.lock()
            .tasks
            .iter()
            .map(|(name, e)| {
                let silent = now.duration_since(e.last_beat);
                TaskStatus {
                    name: name.clone(),
                    healthy: silent <= e.max_silence,
                    silent_secs: silent.as_secs(),
                    max_silence_secs: e.max_silence.as_secs(),
                    restarts: e.restarts,
                    last_ok: e.last_ok,
                    last_error: e.last_error.clone(),
                }
            })
            .collect()
    }

    pub fn unhealthy(&self) -> Vec<String> {
        self.statuses()
            .into_iter()
            .filter(|s| !s.healthy)
            .map(|s| s.name)
            .collect()
    }

    /// Debug hook: make a task hang at its next checkpoint to test the watchdog.
    pub fn stall(&self, name: &str) {
        self.lock().stalled.insert(name.to_string());
    }

    /// Called by tasks at loop boundaries. Never returns while the task is stalled.
    pub async fn checkpoint(&self, name: &str) {
        while self.lock().stalled.contains(name) {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    }
}

/// Runs `make()` forever: a returned error or a panic is logged, counted and
/// the task restarted with capped exponential backoff.
pub fn supervise<F, Fut>(health: Health, name: &'static str, max_silence: Duration, make: F)
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    health.register(name, max_silence);
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        loop {
            let started = Instant::now();
            let outcome = tokio::spawn(make()).await;
            let err = match outcome {
                Ok(Ok(())) => {
                    tracing::warn!(task = name, "task exited; restarting");
                    "exited".to_string()
                }
                Ok(Err(e)) => {
                    tracing::error!(task = name, error = %format!("{e:#}"), "task failed; restarting");
                    format!("{e:#}")
                }
                Err(join) => {
                    tracing::error!(task = name, error = %join, "task panicked; restarting");
                    format!("panic: {join}")
                }
            };
            health.restarted(name, err);
            if started.elapsed() > Duration::from_secs(300) {
                backoff = Duration::from_secs(1);
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
            health.register(name, max_silence);
        }
    });
}

/// Pets systemd's watchdog only while every registered task is healthy. On a
/// development machine without systemd the notify call is a no-op.
pub async fn watchdog_loop(health: Health, interval: Duration) {
    let _ = sd_notify::notify(false, &[sd_notify::NotifyState::Ready]);
    let mut was_healthy = true;
    loop {
        let bad = health.unhealthy();
        if bad.is_empty() {
            let _ = sd_notify::notify(false, &[sd_notify::NotifyState::Watchdog]);
            if !was_healthy {
                tracing::info!("all tasks healthy again; watchdog resumed");
            }
            was_healthy = true;
        } else {
            if was_healthy {
                tracing::error!(tasks = ?bad, "tasks unresponsive; withholding watchdog so systemd restarts us");
            }
            was_healthy = false;
        }
        tokio::time::sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn silent_task_becomes_unhealthy() {
        let h = Health::default();
        h.register("a", Duration::from_millis(30));
        h.register("b", Duration::from_secs(60));
        assert!(h.unhealthy().is_empty());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(h.unhealthy(), vec!["a".to_string()]);
        h.beat("a");
        assert!(h.unhealthy().is_empty());
    }

    #[tokio::test]
    async fn supervisor_restarts_panicking_task() {
        let h = Health::default();
        let runs = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let r = runs.clone();
        supervise(h.clone(), "p", Duration::from_secs(60), move || {
            let r = r.clone();
            async move {
                if r.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    #[allow(clippy::panic)]
                    {
                        panic!("boom");
                    }
                }
                std::future::pending::<()>().await;
                Ok(())
            }
        });
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 2);
        let st = h.statuses();
        assert_eq!(st[0].restarts, 1);
        assert!(st[0].last_error.as_deref().unwrap_or("").contains("panic"));
    }
}
