//! Deterministic weather for local development, matching the design mockup:
//! sunny 25–31° yesterday, rain 21–26° today, cloudy 20–25° tomorrow.

use chrono::{DateTime, Duration, FixedOffset};
use common::weather::{DailyForecast, Warning, WeatherSnapshot};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct MockWeather {
    /// Warnings injected through the debug API.
    pub warnings: Arc<Mutex<Vec<Warning>>>,
}

impl MockWeather {
    pub fn snapshot(&self, now: DateTime<FixedOffset>) -> WeatherSnapshot {
        let today = now.date_naive();
        let plan: [(i64, i32, i32, &str, &str, &str, u32); 5] = [
            (-1, 25, 31, "晴", "晴", "100", 10),
            (0, 21, 26, "中雨", "阴", "306", 70),
            (1, 20, 25, "多云", "多云", "101", 20),
            (2, 22, 28, "晴", "多云", "100", 0),
            (3, 23, 29, "多云", "阵雨", "101", 45),
        ];
        let daily = plan
            .iter()
            .filter_map(|&(off, lo, hi, d, n, icon, pop)| {
                Some(DailyForecast {
                    date: today.checked_add_signed(Duration::days(off))?,
                    temp_max: hi,
                    temp_min: lo,
                    text_day: d.into(),
                    text_night: n.into(),
                    icon_day: icon.into(),
                    precip_prob: Some(pop),
                    precip_mm: if pop >= 40 { 6.5 } else { 0.0 },
                    humidity: 85,
                    wind_dir: "东北风".into(),
                    wind_scale: "3".into(),
                })
            })
            .collect();
        let warnings = self
            .warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        WeatherSnapshot {
            fetched_at: now,
            daily,
            air: Some("优".into()),
            warnings,
        }
    }
}
