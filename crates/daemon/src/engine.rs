//! The scheduler. Each loop re-derives "what should be on screen now" from the
//! wall clock, compares it with what was last handled (persisted), and acts.
//! It never sleeps longer than a minute, so clock jumps and missed events are
//! picked up promptly.

use crate::app::App;
use crate::schedule::{self, Event, Slot};
use crate::state::{Mode, Shown};
use anyhow::Context;
use chrono::{Duration, NaiveDateTime};
use common::weather::Warning;
use common::Frame;
use render::dashboard::{self, DashboardInput, DashboardModel, View};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

pub const TASK: &str = "engine";

/// How long a fresh start waits for NTP before trusting the clock anyway
/// (offline boots still have to show something).
const CLOCK_SYNC_WAIT: std::time::Duration = std::time::Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    /// Re-run whatever the schedule says right now.
    Auto,
    Morning,
    Evening,
    Photo,
}

#[derive(Debug)]
pub enum Command {
    Refresh(Target),
    Alert(Vec<Warning>),
    /// Show a prepared frame (test patterns).
    Show(Box<Frame>, String),
    /// Re-evaluate immediately (e.g. after a mode or clock change).
    Wake,
}

/// Builds the dashboard model for `view` at `now` from cached data.
pub fn dashboard_model(app: &App, view: View, now: NaiveDateTime) -> DashboardModel {
    let cfg = app.config();
    let snap = app.weather.latest();
    let target_date = dashboard::target_date(view, now);
    let ref_date = target_date.pred_opt().unwrap_or(target_date);
    let target = snap.as_ref().and_then(|s| s.day(target_date).cloned());
    let reference = app
        .weather
        .history(ref_date)
        .or_else(|| snap.as_ref().and_then(|s| s.day(ref_date).cloned()));
    let holidays = app.holidays();
    let warnings = snap
        .as_ref()
        .map(|s| s.warnings.clone())
        .unwrap_or_default();
    let input = DashboardInput {
        view,
        now,
        city: &cfg.weather.city_name,
        target: target.as_ref(),
        reference: reference.as_ref(),
        fetched_at: snap
            .as_ref()
            .map(|s| s.fetched_at.with_timezone(&app.clock.tz()).naive_local()),
        air: snap.as_ref().and_then(|s| s.air.as_deref()),
        warnings: &warnings,
        holidays: &holidays,
        thresholds: &cfg.thresholds,
    };
    dashboard::build_model(&input)
}

async fn render_dashboard(app: &Arc<App>, view: View, now: NaiveDateTime) -> anyhow::Result<Frame> {
    // Right after boot the first fetch may still be in flight; give it a moment
    // rather than drawing an empty dashboard for the rest of the slot.
    for _ in 0..30 {
        if app.weather.latest().is_some() {
            break;
        }
        app.health.beat(TASK);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    let model = dashboard_model(app, view, now);
    let fonts = app.fonts.clone();
    Ok(tokio::task::spawn_blocking(move || dashboard::render(&model, &fonts)).await?)
}

fn view_label(view: View) -> &'static str {
    match view {
        View::Morning => "dashboard-morning",
        View::Evening => "dashboard-evening",
    }
}

async fn show_dashboard(app: &Arc<App>, view: View, now: NaiveDateTime) -> anyhow::Result<String> {
    let frame = render_dashboard(app, view, now).await?;
    app.display.show(frame, view_label(view)).await?;
    // Warnings visible on this dashboard need no separate interruption.
    if let Some(s) = app.weather.latest() {
        let ids: Vec<String> = s.warnings.iter().map(|w| w.id.clone()).collect();
        app.update_state(|st| remember_warnings(&mut st.shown_warnings, ids));
    }
    Ok(view_label(view).to_string())
}

fn remember_warnings(seen: &mut Vec<String>, ids: Vec<String>) {
    for id in ids {
        if !seen.contains(&id) {
            seen.push(id);
        }
    }
    let excess = seen.len().saturating_sub(100);
    seen.drain(..excess);
}

async fn show_photo(app: &Arc<App>, now: NaiveDateTime) -> anyhow::Result<String> {
    let cfg = app.config();
    let last = app
        .state()
        .shown
        .as_ref()
        .and_then(|s| s.content.strip_prefix("photo:").map(str::to_string));
    let mut deck = app.state().photo_deck.clone();
    let next = app
        .photos
        .next_from_deck(&mut deck, cfg.photo.favorite_weight, last.as_deref());
    app.update_state(|s| s.photo_deck = deck);
    match next {
        Some(id) => {
            let frame = app
                .photos
                .frame(&id)
                .with_context(|| format!("load photo {id}"))?;
            let label = format!("photo:{id}");
            app.display.show(frame, label.clone()).await?;
            Ok(label)
        }
        None => {
            tracing::info!("no photos to show; showing the dashboard instead");
            show_dashboard(app, View::Morning, now).await
        }
    }
}

fn effective_slot(mode: Mode, slot: Slot) -> Slot {
    match (mode, slot) {
        (Mode::Auto, s) => s,
        (Mode::Dashboard, Slot::Evening) => Slot::Evening,
        (Mode::Dashboard, _) => Slot::Morning,
        (Mode::Photo, _) => Slot::Photo,
    }
}

async fn show_slot(app: &Arc<App>, slot: Slot, now: NaiveDateTime) -> anyhow::Result<String> {
    match slot {
        Slot::Morning => show_dashboard(app, View::Morning, now).await,
        Slot::Evening => show_dashboard(app, View::Evening, now).await,
        Slot::Photo => show_photo(app, now).await,
    }
}

fn record_shown(app: &App, content: String, now: NaiveDateTime, event: Option<&Event>) {
    app.update_state(|s| {
        s.shown = Some(Shown { content, at: now });
        if let Some(e) = event {
            s.handled_event = Some(e.key());
        }
    });
}

/// What the scheduler wants to do at `now`, if anything.
pub fn due_event(app: &App, now: NaiveDateTime) -> Option<Event> {
    let cfg = app.config();
    let ev = schedule::current(&cfg.schedule, &app.holidays(), now)?;
    let st = app.state();
    let handled = st.handled_event.as_deref() == Some(ev.key().as_str());
    // Never leave one image up for more than a day (ghosting).
    let too_old = st
        .shown
        .as_ref()
        .is_none_or(|s| now - s.at > Duration::hours(24));
    (!handled || too_old).then_some(ev)
}

pub async fn run(app: Arc<App>, rx: Arc<Mutex<mpsc::Receiver<Command>>>) -> anyhow::Result<()> {
    let mut rx = rx.lock().await;
    let mut failures: u32 = 0;
    let mut retry_at: Option<NaiveDateTime> = None;
    let started = std::time::Instant::now();
    let mut clock_trusted = false;
    loop {
        app.health.checkpoint(TASK).await;
        app.health.beat(TASK);
        let now = app.clock.local();
        if !clock_trusted {
            clock_trusted = crate::clock::system_synced()
                || app.clock.offset() != 0
                || started.elapsed() >= CLOCK_SYNC_WAIT;
            if clock_trusted {
                tracing::info!(%now, synced = crate::clock::system_synced(), "clock trusted; scheduling starts");
            }
        }

        // The setup code stays up until provisioning ends (which re-runs the schedule).
        if clock_trusted
            && retry_at.is_none_or(|t| now >= t)
            && !app.provision.holds_display()
            && app.power.pending().is_none()
        {
            if let Some(ev) = due_event(&app, now) {
                let mode = app.state().mode;
                let slot = effective_slot(mode, ev.slot);
                tracing::info!(event = %ev.key(), ?slot, "scheduled refresh");
                match show_slot(&app, slot, now).await {
                    Ok(content) => {
                        record_shown(&app, content, now, Some(&ev));
                        app.health.ok(TASK);
                        failures = 0;
                        retry_at = None;
                    }
                    Err(e) => {
                        failures += 1;
                        let wait = Duration::minutes(1 << failures.min(5));
                        retry_at = Some(now + wait);
                        app.health.error(TASK, format!("{e:#}"));
                        tracing::error!(error = %format!("{e:#}"), retry_in_min = wait.num_minutes(), "scheduled refresh failed");
                    }
                }
            }
        }

        let cfg = app.config();
        let now = app.clock.local();
        let until_next = schedule::next(&cfg.schedule, &app.holidays(), now)
            .map(|e| (e.at - now).to_std().unwrap_or_default())
            .unwrap_or(std::time::Duration::from_secs(60));
        let cap = if clock_trusted { 60 } else { 5 };
        let wait = until_next.min(std::time::Duration::from_secs(cap))
            + std::time::Duration::from_millis(200);

        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(cmd) => handle(&app, cmd).await,
                None => anyhow::bail!("command channel closed"),
            },
            _ = tokio::time::sleep(wait) => {}
        }
    }
}

async fn handle(app: &Arc<App>, cmd: Command) {
    let now = app.clock.local();
    if let Some(action) = app.power.pending() {
        if !matches!(cmd, Command::Wake) {
            tracing::warn!(?action, "ignoring panel request during power action");
        }
        return;
    }
    let result = match cmd {
        Command::Wake => return,
        Command::Refresh(Target::Auto) => {
            let cfg = app.config();
            match schedule::current(&cfg.schedule, &app.holidays(), now) {
                Some(ev) => {
                    let slot = effective_slot(app.state().mode, ev.slot);
                    show_slot(app, slot, now)
                        .await
                        .map(|c| record_shown(app, c, now, Some(&ev)))
                }
                None => show_slot(app, Slot::Morning, now)
                    .await
                    .map(|c| record_shown(app, c, now, None)),
            }
        }
        Command::Refresh(t) => {
            let slot = match t {
                Target::Morning | Target::Auto => Slot::Morning,
                Target::Evening => Slot::Evening,
                Target::Photo => Slot::Photo,
            };
            show_slot(app, slot, now)
                .await
                .map(|c| record_shown(app, c, now, None))
        }
        Command::Alert(ws) => {
            tracing::warn!(
                count = ws.len(),
                "weather warning: interrupting with the dashboard"
            );
            show_dashboard(app, View::Morning, now)
                .await
                .map(|c| record_shown(app, c, now, None))
        }
        Command::Show(frame, label) => app
            .display
            .show(*frame, label.clone())
            .await
            .map(|_| record_shown(app, label, now, None)),
    };
    if let Err(e) = result {
        tracing::error!(error = %format!("{e:#}"), "requested refresh failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_override_slots() {
        assert_eq!(effective_slot(Mode::Auto, Slot::Photo), Slot::Photo);
        assert_eq!(effective_slot(Mode::Dashboard, Slot::Photo), Slot::Morning);
        assert_eq!(
            effective_slot(Mode::Dashboard, Slot::Evening),
            Slot::Evening
        );
        assert_eq!(effective_slot(Mode::Photo, Slot::Morning), Slot::Photo);
    }

    #[test]
    fn warning_memory_is_bounded() {
        let mut seen = Vec::new();
        remember_warnings(&mut seen, (0..150).map(|i| i.to_string()).collect());
        remember_warnings(&mut seen, vec!["149".into()]);
        assert_eq!(seen.len(), 100);
        assert_eq!(seen.last().map(String::as_str), Some("149"));
    }
}
