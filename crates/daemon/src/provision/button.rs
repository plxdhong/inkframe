//! Optional long-press button (active low, internal pull-up) that starts
//! provisioning. The e-Paper HAT has no key, so this is any spare GPIO wired
//! to ground through a push button; configure `provision.button_pin`.

use super::Reason;
use crate::app::App;
use anyhow::Context;
use gpiocdev::line::{Bias, Value};
use gpiocdev::Request;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const TASK: &str = "button";

pub async fn run(app: Arc<App>) -> anyhow::Result<()> {
    let cfg = app.config();
    let Some(pin) = cfg.provision.button_pin else {
        // Nothing to watch; stay alive so the supervisor is satisfied.
        loop {
            app.health.beat(TASK);
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    };
    let hold = Duration::from_secs(cfg.provision.button_hold_secs.max(1));
    let req = Request::builder()
        .on_chip(&cfg.display.gpio_chip)
        .with_consumer("inkframe-button")
        .with_line(pin)
        .as_input()
        .with_bias(Bias::PullUp)
        .as_active_low()
        .request()
        .with_context(|| format!("request button gpio {pin}"))?;
    let mut pressed_since: Option<Instant> = None;
    let mut fired = false;
    loop {
        app.health.checkpoint(TASK).await;
        app.health.beat(TASK);
        let down = req.value(pin).context("read button")? == Value::Active;
        match (down, pressed_since) {
            (true, None) => pressed_since = Some(Instant::now()),
            (true, Some(t)) if !fired && t.elapsed() >= hold => {
                fired = true;
                tracing::info!(pin, "button held: starting provisioning");
                app.provision.start(Reason::Button);
            }
            (false, _) => {
                pressed_since = None;
                fired = false;
            }
            _ => {}
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
