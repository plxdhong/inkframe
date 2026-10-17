//! Weather fetching, the on-disk cache, and the daily history that supplies
//! "yesterday" (the provider has no history endpoint we rely on).

pub mod mock;
pub mod qweather;

use anyhow::Context;
use chrono::{DateTime, FixedOffset, NaiveDate};
use common::fsutil::write_atomic;
use common::weather::{DailyForecast, WeatherSnapshot};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub enum Provider {
    QWeather(Box<qweather::Client>),
    Mock(mock::MockWeather),
    /// Misconfigured: every fetch fails with this reason. The daemon keeps
    /// running (a startup error here would crash-loop into a reboot loop).
    Unavailable(String),
}

impl Provider {
    pub async fn fetch(&self, now: DateTime<FixedOffset>) -> anyhow::Result<WeatherSnapshot> {
        match self {
            Provider::QWeather(c) => c.fetch(now).await,
            Provider::Mock(m) => Ok(m.snapshot(now)),
            Provider::Unavailable(why) => anyhow::bail!("weather not configured: {why}"),
        }
    }
}

/// Latest snapshot (memory + `weather.json`) and per-day records under `history/`.
pub struct WeatherStore {
    dir: PathBuf,
    latest: Mutex<Option<Arc<WeatherSnapshot>>>,
}

impl WeatherStore {
    pub fn open(data_dir: &std::path::Path) -> Self {
        let dir = data_dir.join("weather");
        let latest = std::fs::read(dir.join("latest.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<WeatherSnapshot>(&b).ok())
            .map(Arc::new);
        Self {
            dir,
            latest: Mutex::new(latest),
        }
    }

    pub fn latest(&self) -> Option<Arc<WeatherSnapshot>> {
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Saves a fresh snapshot and records today's forecast as today's history.
    /// Earlier days a provider happens to return are only stored if missing.
    pub fn store(&self, snap: WeatherSnapshot, today: NaiveDate) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec_pretty(&snap)?;
        write_atomic(&self.dir.join("latest.json"), &bytes).context("write weather cache")?;
        for d in &snap.daily {
            let path = self.history_path(d.date);
            if d.date == today || (d.date < today && !path.exists()) {
                write_atomic(&path, &serde_json::to_vec_pretty(d)?)
                    .context("write weather history")?;
            }
        }
        self.prune_history(today);
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(snap));
        Ok(())
    }

    fn history_path(&self, date: NaiveDate) -> PathBuf {
        self.dir.join("history").join(format!("{date}.json"))
    }

    pub fn history(&self, date: NaiveDate) -> Option<DailyForecast> {
        let bytes = std::fs::read(self.history_path(date)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn prune_history(&self, today: NaiveDate) {
        let Ok(rd) = std::fs::read_dir(self.dir.join("history")) else {
            return;
        };
        for entry in rd.flatten() {
            let name = entry
                .file_name()
                .to_string_lossy()
                .trim_end_matches(".json")
                .to_string();
            if let Ok(d) = NaiveDate::parse_from_str(&name, "%Y-%m-%d") {
                if (today - d).num_days() > 14 {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn today_overwrites_history_but_past_days_do_not() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let store = WeatherStore::open(tmp.path());
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-15T07:00:00+08:00")?;
        let mock = mock::MockWeather::default();
        let today = now.date_naive();
        store.store(mock.snapshot(now), today)?;
        let yesterday = today.pred_opt().ok_or_else(|| anyhow::anyhow!("date"))?;
        assert_eq!(store.history(yesterday).map(|d| d.temp_max), Some(31));

        let mut snap = mock.snapshot(now);
        for d in &mut snap.daily {
            d.temp_max += 10;
        }
        store.store(snap, today)?;
        assert_eq!(store.history(yesterday).map(|d| d.temp_max), Some(31));
        assert_eq!(store.history(today).map(|d| d.temp_max), Some(36));
        // Reopening reads the cache back.
        assert!(WeatherStore::open(tmp.path()).latest().is_some());
        Ok(())
    }
}
