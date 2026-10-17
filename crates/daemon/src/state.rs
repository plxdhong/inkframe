//! Small persisted state: what is on the panel and which event produced it.

use chrono::NaiveDateTime;
use common::fsutil::write_atomic;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Follow the timetable.
    #[default]
    Auto,
    /// Every event shows the dashboard.
    Dashboard,
    /// Every event shows a photo.
    Photo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Shown {
    /// "dashboard-morning", "dashboard-evening", "photo:<id>", "pattern:colors", ...
    pub content: String,
    pub at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct State {
    pub mode: Mode,
    /// Key of the last scheduled event that was acted upon.
    pub handled_event: Option<String>,
    pub shown: Option<Shown>,
    /// Warning ids already interrupted for, so each one is shown once.
    pub shown_warnings: Vec<String>,
    /// Photo ids still to show in the current round.
    pub photo_deck: Vec<String>,
    /// Reboots for lost connectivity since the network was last seen up;
    /// stretches the next reboot threshold.
    pub net_reboots: u32,
}

pub struct StateFile {
    path: PathBuf,
}

impl StateFile {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("state.json"),
        }
    }

    pub fn load(&self) -> State {
        match std::fs::read(&self.path) {
            Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "state file unreadable; starting fresh");
                State::default()
            }),
            Err(_) => State::default(),
        }
    }

    pub fn save(&self, s: &State) -> anyhow::Result<()> {
        write_atomic(&self.path, &serde_json::to_vec_pretty(s)?)?;
        Ok(())
    }
}
