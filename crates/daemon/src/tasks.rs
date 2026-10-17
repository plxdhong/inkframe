//! Background loops: weather fetch (with dashboard prefetch and warning
//! interrupts), holiday data refresh, and network self-healing.

use crate::app::App;
use crate::engine::Command;
use crate::network;
use crate::schedule;
use chrono::{Datelike, Duration, NaiveDateTime};
use common::calendar::HolidayCalendar;
use common::fsutil::write_atomic;
use std::path::Path;
use std::sync::Arc;

pub const WEATHER: &str = "weather";
pub const HOLIDAYS: &str = "holidays";
pub const NETWORK: &str = "network";

const TICK: std::time::Duration = std::time::Duration::from_secs(30);

/// Whether a fetch is due: no data, data older than the poll interval, or we
/// are inside the prefetch window of the next dashboard and have not fetched
/// since it opened.
pub fn weather_due(
    last: Option<NaiveDateTime>,
    now: NaiveDateTime,
    prefetch_from: Option<NaiveDateTime>,
    poll: Duration,
) -> bool {
    let Some(last) = last else { return true };
    if now - last >= poll {
        return true;
    }
    matches!(prefetch_from, Some(p) if now >= p && last < p)
}

pub async fn weather_loop(app: Arc<App>) -> anyhow::Result<()> {
    let mut backoff = Duration::seconds(30);
    let mut retry_at: Option<NaiveDateTime> = None;
    loop {
        app.health.checkpoint(WEATHER).await;
        app.health.beat(WEATHER);
        let cfg = app.config();
        let now = app.clock.local();
        let last = app
            .weather
            .latest()
            .map(|s| s.fetched_at.with_timezone(&app.clock.tz()).naive_local());
        let prefetch_from = schedule::next_dashboard(&cfg.schedule, &app.holidays(), now)
            .map(|e| e.at - Duration::minutes(cfg.weather.prefetch_minutes))
            // The window stays open until the event itself.
            .filter(|p| *p <= now + Duration::minutes(cfg.weather.prefetch_minutes));
        let due = weather_due(
            last,
            now,
            prefetch_from,
            Duration::minutes(cfg.weather.poll_minutes),
        );
        if due && retry_at.is_none_or(|t| now >= t) {
            let fetched_at = app.clock.now().fixed_offset();
            match app.provider.fetch(fetched_at).await {
                Ok(snap) => {
                    let warnings = snap.warnings.clone();
                    app.weather.store(snap, now.date())?;
                    app.health.ok(WEATHER);
                    backoff = Duration::seconds(30);
                    retry_at = None;
                    tracing::info!(
                        days = app.weather.latest().map(|s| s.daily.len()).unwrap_or(0),
                        "weather updated"
                    );
                    let unseen: Vec<_> = {
                        let st = app.state();
                        warnings
                            .into_iter()
                            .filter(|w| !st.shown_warnings.contains(&w.id))
                            .collect()
                    };
                    if !unseen.is_empty() && schedule::in_alert_window(&cfg.schedule, now) {
                        let _ = app.tx.send(Command::Alert(unseen)).await;
                    }
                }
                Err(e) => {
                    app.health.error(WEATHER, format!("{e:#}"));
                    tracing::warn!(error = %format!("{e:#}"), retry_in_s = backoff.num_seconds(), "weather fetch failed");
                    retry_at = Some(now + backoff);
                    backoff = (backoff * 2).min(Duration::minutes(5));
                }
            }
        }
        tokio::time::sleep(TICK).await;
    }
}

pub fn load_cached_holidays(data_dir: &Path) -> HolidayCalendar {
    let mut cal = HolidayCalendar::new();
    if let Ok(rd) = std::fs::read_dir(data_dir.join("holidays")) {
        for e in rd.flatten() {
            if let Ok(text) = std::fs::read_to_string(e.path()) {
                if let Err(err) = cal.add_year_json(&text) {
                    tracing::warn!(file = %e.path().display(), error = %err, "ignoring bad holiday file");
                }
            }
        }
    }
    cal
}

pub async fn holiday_loop(app: Arc<App>) -> anyhow::Result<()> {
    loop {
        app.health.checkpoint(HOLIDAYS).await;
        let cfg = app.config();
        if !cfg.holiday.url_template.is_empty() {
            let year = app.clock.local().year();
            let mut any_ok = false;
            for y in [year, year + 1] {
                let sources =
                    std::iter::once(&cfg.holiday.url_template).chain(cfg.holiday.mirrors.iter());
                for template in sources.filter(|t| !t.is_empty()) {
                    let url = template.replace("{year}", &y.to_string());
                    app.health.beat(HOLIDAYS);
                    match fetch_holiday_year(&app, &url).await {
                        Ok(text) => {
                            write_atomic(
                                &cfg.data_dir.join("holidays").join(format!("{y}.json")),
                                text.as_bytes(),
                            )?;
                            any_ok = true;
                            break;
                        }
                        // Next year's file usually appears in November.
                        Err(e) => {
                            tracing::debug!(year = y, %url, error = %format!("{e:#}"), "holiday data not fetched")
                        }
                    }
                }
            }
            app.set_holidays(load_cached_holidays(&cfg.data_dir));
            if any_ok {
                app.health.ok(HOLIDAYS);
            } else {
                app.health.error(
                    HOLIDAYS,
                    "no holiday data fetched; using cache and built-in tables",
                );
            }
        }
        // Sleep in short steps so the heartbeat stays fresh.
        let until = std::time::Instant::now()
            + std::time::Duration::from_secs(cfg.holiday.refresh_hours.max(1) as u64 * 3600);
        while std::time::Instant::now() < until {
            app.health.beat(HOLIDAYS);
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    }
}

async fn fetch_holiday_year(app: &App, url: &str) -> anyhow::Result<String> {
    let resp = app.http.get(url).send().await?.error_for_status()?;
    let text = resp.text().await?;
    // Validate before caching.
    HolidayCalendar::new().add_year_json(&text)?;
    Ok(text)
}

pub async fn network_loop(app: Arc<App>) -> anyhow::Result<()> {
    let mut powersave_checked = false;
    loop {
        app.health.checkpoint(NETWORK).await;
        app.health.beat(NETWORK);
        let cfg = app.config();
        let net = &cfg.network;
        if net.enabled {
            // Once per task start: any profile created since (by provisioning,
            // nmcli by hand, or another tool) is caught on the next restart.
            if !powersave_checked && !app.provision.active() {
                powersave_checked = true;
                if network::enforce_powersave_off(net).await {
                    tracing::warn!("re-activating Wi-Fi so power saving off takes effect");
                    network::run_action(net, network::Action::Reconnect).await;
                }
                app.health.beat(NETWORK);
            }
            let online = network::probe(&net.probes).await;
            let failures = {
                let mut st = app.net.lock().unwrap_or_else(|e| e.into_inner());
                st.online = online;
                st.consecutive_failures = if online {
                    0
                } else {
                    st.consecutive_failures + 1
                };
                st.offline_since = if online {
                    None
                } else {
                    st.offline_since.or_else(|| Some(std::time::Instant::now()))
                };
                st.consecutive_failures
            };
            let net_reboots = app.state().net_reboots;
            if online {
                app.health.ok(NETWORK);
                if net_reboots > 0 {
                    tracing::info!(net_reboots, "network back; reboot backoff reset");
                    app.update_state(|s| s.net_reboots = 0);
                }
            }
            let action = network::decide(
                net,
                failures,
                net_reboots,
                network::usb_link_up(&net.usb_interface),
            );
            if action != network::Action::None && app.provision.active() {
                // nmcli may be joining the network the phone just sent; do not
                // restart NetworkManager or reboot underneath it.
                tracing::warn!(
                    ?action,
                    failures,
                    "network unreachable; recovery deferred while provisioning"
                );
            } else if action != network::Action::None {
                tracing::warn!(?action, failures, net_reboots, "network unreachable");
                app.net
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .last_action = Some(action);
                if action == network::Action::Reboot && !net.dry_run {
                    app.update_state(|s| s.net_reboots = s.net_reboots.saturating_add(1));
                }
                network::run_action(net, action).await;
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(
            net.check_interval_secs.max(5),
        ))
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn t(h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 15)
            .and_then(|d| d.and_hms_opt(h, m, 0))
            .unwrap_or_default()
    }

    #[test]
    fn prefetch_window_forces_one_fresh_fetch() {
        let poll = Duration::minutes(60);
        // Fetched at 06:30; the 07:00 dashboard's window opens at 06:50.
        assert!(!weather_due(Some(t(6, 30)), t(6, 45), Some(t(6, 50)), poll));
        assert!(weather_due(Some(t(6, 30)), t(6, 50), Some(t(6, 50)), poll));
        assert!(!weather_due(Some(t(6, 51)), t(6, 55), Some(t(6, 50)), poll));
        assert!(weather_due(None, t(6, 55), None, poll));
        assert!(weather_due(Some(t(5, 0)), t(6, 0), None, poll));
    }
}
