//! Provider-neutral weather data kept by the daemon and consumed by the renderer.

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DailyForecast {
    pub date: NaiveDate,
    pub temp_max: i32,
    pub temp_min: i32,
    pub text_day: String,
    pub text_night: String,
    /// QWeather icon code for the day, e.g. "305".
    pub icon_day: String,
    /// Highest hourly precipitation probability seen for this date, if known.
    pub precip_prob: Option<u32>,
    pub precip_mm: f32,
    pub humidity: u32,
    pub wind_dir: String,
    pub wind_scale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Warning {
    pub id: String,
    pub title: String,
    /// e.g. "暴雨"
    pub type_name: String,
    /// e.g. "Yellow", "Orange", "Red", "Blue"
    pub color: String,
    pub pub_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeatherSnapshot {
    pub fetched_at: DateTime<FixedOffset>,
    pub daily: Vec<DailyForecast>,
    /// Air quality category, e.g. "优".
    pub air: Option<String>,
    pub warnings: Vec<Warning>,
}

impl WeatherSnapshot {
    pub fn day(&self, date: NaiveDate) -> Option<&DailyForecast> {
        self.daily.iter().find(|d| d.date == date)
    }
}
