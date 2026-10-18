//! Remote shutdown and reboot (`POST /api/v1/power`).
//!
//! Shutdown first paints a power-off screen, because the panel keeps its last
//! image without power. It also forgets the handled event so the schedule
//! repaints as soon as the frame is powered on again. Reboot leaves the panel
//! alone: the daemon is back within a minute and the image is still current.

use crate::app::App;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Shutdown,
    Reboot,
}

impl Action {
    fn argv(self) -> [&'static str; 2] {
        match self {
            Action::Shutdown => ["systemctl", "poweroff"],
            Action::Reboot => ["systemctl", "reboot"],
        }
    }
}

/// The action in progress, if any. While set, the engine paints nothing.
#[derive(Default)]
pub struct Power(Mutex<Option<Action>>);

impl Power {
    pub fn pending(&self) -> Option<Action> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Claims the slot; `false` if another action is already running.
    fn begin(&self, action: Action) -> bool {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_some() {
            return false;
        }
        *g = Some(action);
        true
    }

    fn clear(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PowerError {
    #[error("power control is disabled (power.enabled = false)")]
    Disabled,
    #[error("{0:?} already in progress")]
    Busy(Action),
}

/// Starts `action` in the background and returns once it is accepted.
pub fn request(app: &Arc<App>, action: Action) -> Result<(), PowerError> {
    if !app.config().power.enabled {
        return Err(PowerError::Disabled);
    }
    if !app.power.begin(action) {
        return Err(PowerError::Busy(app.power.pending().unwrap_or(action)));
    }
    tracing::warn!(?action, "power action requested");
    let app = app.clone();
    tokio::spawn(async move {
        run(&app, action).await;
        // Only reached when the command failed or in dry-run: resume the schedule.
        app.display.unseal();
        app.power.clear();
        let _ = app.tx.try_send(crate::engine::Command::Wake);
    });
    Ok(())
}

async fn run(app: &Arc<App>, action: Action) {
    if action == Action::Shutdown {
        paint_off_screen(app).await;
    }
    let argv = action.argv();
    let [prog, arg] = argv;
    if app.config().power.dry_run {
        tracing::warn!(cmd = %argv.join(" "), "power action (dry run)");
        return;
    }
    tracing::warn!(cmd = %argv.join(" "), "power action");
    let out = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(prog)
            .arg(arg)
            .kill_on_drop(true)
            .output(),
    )
    .await;
    match out {
        Ok(Ok(o)) if o.status.success() => {
            // systemd is now stopping us; keep the panel quiet until it does.
            tokio::time::sleep(Duration::from_secs(120)).await;
            tracing::error!(?action, "still running two minutes after the power command");
        }
        Ok(Ok(o)) => tracing::error!(
            status = %o.status,
            stderr = %String::from_utf8_lossy(&o.stderr).trim(),
            "power command failed"
        ),
        Ok(Err(e)) => tracing::error!(error = %e, "power command could not start"),
        Err(_) => tracing::error!("power command timed out"),
    }
}

/// Best effort: a failed paint must not keep the frame from shutting down.
async fn paint_off_screen(app: &Arc<App>) {
    let now = app.clock.local();
    let at = now.format("%m-%d %H:%M").to_string();
    let name = app.identity.name.clone();
    let fonts = app.fonts.clone();
    let frame =
        match tokio::task::spawn_blocking(move || render::power::off(&fonts, &name, &at)).await {
            Ok(f) => f,
            Err(e) => {
                tracing::error!(error = %e, "could not render the power-off screen");
                return;
            }
        };
    match app.display.show_final(frame, "power-off").await {
        Ok(()) => app.update_state(|s| {
            s.shown = Some(crate::state::Shown {
                content: "power-off".into(),
                at: now,
            });
            s.handled_event = None;
        }),
        Err(e) => tracing::error!(error = %format!("{e:#}"), "could not paint the power-off screen"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_action_at_a_time() {
        let p = Power::default();
        assert!(p.begin(Action::Shutdown));
        assert!(!p.begin(Action::Reboot));
        assert_eq!(p.pending(), Some(Action::Shutdown));
        p.clear();
        assert!(p.begin(Action::Reboot));
    }
}
