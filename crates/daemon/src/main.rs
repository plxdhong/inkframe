//! inkframe: e-paper photo frame and dashboard daemon.

mod app;
mod clock;
mod device;
mod display;
mod engine;
mod health;
mod http;
mod logs;
mod mdns;
mod network;
mod photos;
mod power;
mod provision;
mod schedule;
mod state;
mod tasks;
mod weather;

use anyhow::Context;
use common::Config;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG: &str = "/data/inkframe/config.toml";

fn parse_args() -> anyhow::Result<PathBuf> {
    let mut args = std::env::args().skip(1);
    let mut path = PathBuf::from(DEFAULT_CONFIG);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-c" | "--config" => {
                path = PathBuf::from(args.next().context("--config needs a path")?)
            }
            "-h" | "--help" => {
                println!("usage: inkframe [--config PATH]   (default {DEFAULT_CONFIG})");
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument {other:?}"),
        }
    }
    Ok(path)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let logs = logs::LogBuffer::default();
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,hyper=warn,reqwest=warn")),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .with(logs.clone())
        .init();

    let cfg_path = parse_args()?;
    let cfg = if cfg_path.exists() {
        Config::load(&cfg_path)?
    } else {
        tracing::warn!(path = %cfg_path.display(), "config not found; writing defaults");
        let c = Config::default();
        c.save(&cfg_path)?;
        c
    };
    let listen = cfg.http.listen.clone();
    let watchdog_interval = Duration::from_secs(cfg.watchdog.notify_interval_secs.max(1));
    let refresh_budget = Duration::from_secs(
        cfg.display.busy_timeout_secs * 5 * (cfg.display.max_retries as u64 + 1) + 180,
    );

    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let app = app::App::new(cfg, cfg_path, logs, tx)?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        backend = %app.display.status().backend,
        now = %app.clock.local(),
        "inkframe starting"
    );

    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let a = app.clone();
    health::supervise(
        app.health.clone(),
        engine::TASK,
        refresh_budget,
        move || engine::run(a.clone(), rx.clone()),
    );
    let a = app.clone();
    health::supervise(
        app.health.clone(),
        tasks::WEATHER,
        Duration::from_secs(300),
        move || tasks::weather_loop(a.clone()),
    );
    let a = app.clone();
    health::supervise(
        app.health.clone(),
        tasks::HOLIDAYS,
        Duration::from_secs(600),
        move || tasks::holiday_loop(a.clone()),
    );
    let a = app.clone();
    let net_silence = Duration::from_secs(app.config().network.check_interval_secs * 3 + 120);
    health::supervise(app.health.clone(), tasks::NETWORK, net_silence, move || {
        tasks::network_loop(a.clone())
    });
    let a = app.clone();
    health::supervise(
        app.health.clone(),
        provision::TASK,
        Duration::from_secs(120),
        move || provision::run(a.clone()),
    );
    #[cfg(all(feature = "hw", target_os = "linux"))]
    {
        let a = app.clone();
        health::supervise(
            app.health.clone(),
            provision::button::TASK,
            Duration::from_secs(120),
            move || provision::button::run(a.clone()),
        );
    }
    tokio::spawn(health::watchdog_loop(app.health.clone(), watchdog_interval));
    // Kept alive for the life of the process; failure only costs discovery.
    let _mdns = mdns::advertise(&app);

    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .with_context(|| format!("bind {listen}"))?;
    tracing::info!(addr = %listen, "http listening");
    axum::serve(listener, http::router(app))
        .with_graceful_shutdown(shutdown())
        .await
        .context("http server")?;
    tracing::info!("shutting down");
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
