//! The panel lives on its own OS thread: refreshes block for ~25 s and the
//! SPI/GPIO calls are synchronous. Jobs arrive over a channel; failures are
//! retried with a hardware reset, and repeated failure marks the panel unhealthy.

use crate::health::Health;
use chrono::{DateTime, Utc};
use common::config::{Backend, DisplayConfig};
use common::Frame;
use epd::{Epd, Hal, MockHal};
use render::Palette;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

pub const TASK: &str = "display";

struct Job {
    frame: Frame,
    label: String,
    reply: oneshot::Sender<Result<(), String>>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DisplayStatus {
    pub backend: String,
    pub busy: bool,
    pub label: Option<String>,
    pub last_refresh: Option<DateTime<Utc>>,
    pub last_duration_ms: Option<u128>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
    pub total_refreshes: u64,
}

#[derive(Default)]
struct Shared {
    status: DisplayStatus,
    frame: Option<Frame>,
    preview: Option<Arc<Vec<u8>>>,
}

#[derive(Clone)]
pub struct Display {
    tx: mpsc::Sender<Job>,
    shared: Arc<Mutex<Shared>>,
    job_timeout: Duration,
}

fn open_hal(cfg: &DisplayConfig) -> anyhow::Result<Box<dyn Hal>> {
    match cfg.backend {
        Backend::Mock => Ok(Box::new(MockHal::new())),
        #[cfg(all(target_os = "linux", feature = "hw"))]
        Backend::Hardware => Ok(Box::new(epd::LinuxHal::open(cfg)?)),
        #[cfg(not(all(target_os = "linux", feature = "hw")))]
        Backend::Hardware => anyhow::bail!(
            "hardware backend needs Linux and the `hw` feature; set display.backend = \"mock\""
        ),
    }
}

impl Display {
    /// `last_frame` keeps what the panel shows across restarts, for the preview.
    pub fn start(
        cfg: &DisplayConfig,
        health: Health,
        palette: Palette,
        last_frame: PathBuf,
    ) -> anyhow::Result<Self> {
        let hal = open_hal(cfg)?;
        let busy_timeout = Duration::from_secs(cfg.busy_timeout_secs);
        let max_retries = cfg.max_retries;
        let simulate = cfg.backend == Backend::Mock;
        let (tx, rx) = mpsc::channel::<Job>();
        let shared = Arc::new(Mutex::new(Shared {
            status: DisplayStatus {
                backend: format!("{:?}", cfg.backend).to_lowercase(),
                ..Default::default()
            },
            ..Default::default()
        }));
        if let Some(frame) = std::fs::read(&last_frame)
            .ok()
            .and_then(|b| Frame::unpack(&b))
        {
            let mut g = lock(&shared);
            g.preview = render::preview_png(&frame, &palette).ok().map(Arc::new);
            g.frame = Some(frame);
        }
        // A refresh is 4 BUSY waits plus transfer; allow every retry to time out.
        let job_timeout = busy_timeout * 5 * (max_retries + 1) + Duration::from_secs(60);
        health.register(TASK, job_timeout + Duration::from_secs(60));
        let sh = shared.clone();
        std::thread::Builder::new().name("epd".into()).spawn(move || {
            let mut epd = Epd::new(hal, busy_timeout);
            loop {
                health.beat(TASK);
                let job = match rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(j) => j,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                };
                lock(&sh).status.busy = true;
                let started = std::time::Instant::now();
                let mut result = Err(String::new());
                for attempt in 0..=max_retries {
                    if attempt > 0 {
                        tracing::warn!(attempt, "retrying refresh after hardware reset");
                        let _ = epd.reset();
                    }
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| epd.display(&job.frame))) {
                        Ok(Ok(())) => {
                            result = Ok(());
                            break;
                        }
                        Ok(Err(e)) => result = Err(e.to_string()),
                        Err(_) => result = Err("driver panicked".into()),
                    }
                    health.beat(TASK);
                }
                if simulate {
                    // Roughly what the eye sees on the real panel; keeps the "busy" path visible.
                    std::thread::sleep(Duration::from_millis(1500));
                }
                let mut g = lock(&sh);
                g.status.busy = false;
                g.status.last_duration_ms = Some(started.elapsed().as_millis());
                match &result {
                    Ok(()) => {
                        g.status.last_refresh = Some(Utc::now());
                        g.status.label = Some(job.label.clone());
                        g.status.last_error = None;
                        g.status.consecutive_failures = 0;
                        g.status.total_refreshes += 1;
                        g.preview = render::preview_png(&job.frame, &palette).ok().map(Arc::new);
                        if let Err(e) = common::fsutil::write_atomic(&last_frame, &job.frame.pack()) {
                            tracing::warn!(error = %e, "could not persist last frame");
                        }
                        g.frame = Some(job.frame);
                        health.ok(TASK);
                        tracing::info!(label = %job.label, ms = started.elapsed().as_millis() as u64, "panel refreshed");
                    }
                    Err(e) => {
                        g.status.last_error = Some(e.clone());
                        g.status.consecutive_failures += 1;
                        health.error(TASK, e);
                        tracing::error!(label = %job.label, error = %e, "panel refresh failed");
                    }
                }
                drop(g);
                let _ = job.reply.send(result);
            }
        })?;
        Ok(Self {
            tx,
            shared,
            job_timeout,
        })
    }

    pub async fn show(&self, frame: Frame, label: impl Into<String>) -> anyhow::Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Job {
                frame,
                label: label.into(),
                reply,
            })
            .map_err(|_| anyhow::anyhow!("display thread is gone"))?;
        match tokio::time::timeout(self.job_timeout, rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(e))) => Err(anyhow::anyhow!("refresh failed: {e}")),
            Ok(Err(_)) => Err(anyhow::anyhow!("display thread dropped the job")),
            Err(_) => Err(anyhow::anyhow!(
                "refresh did not finish in {:?}",
                self.job_timeout
            )),
        }
    }

    pub fn status(&self) -> DisplayStatus {
        lock(&self.shared).status.clone()
    }

    pub fn preview(&self) -> Option<Arc<Vec<u8>>> {
        lock(&self.shared).preview.clone()
    }
}

fn lock(m: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
