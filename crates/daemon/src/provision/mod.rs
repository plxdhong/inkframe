//! BLE Wi-Fi provisioning (plan P3). Protocol: docs/PROVISIONING.md.
//!
//! A *run* starts when the device has no Wi-Fi profile, has been offline for
//! a while, the button is held, or the API asks. While a run is active the
//! transport (BLE, or a TCP simulator for development) accepts one phone at a
//! time. The setup code is drawn on the panel — immediately for a device that
//! was never configured, otherwise only once a phone actually says hello, so
//! a short router outage does not repaint the screen.

pub mod crypto;
pub mod frame;
pub mod session;
pub mod sim;
pub mod wifi;

#[cfg(all(feature = "ble", target_os = "linux"))]
mod ble;
#[cfg(all(feature = "hw", target_os = "linux"))]
pub mod button;

use crate::app::App;
use crate::engine::{Command, Target};
use common::config::{ProvisionConfig, ProvisionTransport};
use serde::Serialize;
use session::{Event, Reply, Request, Session};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use wifi::{Wifi, WifiState};

pub const TASK: &str = "provision";

/// GATT layout; the app hard-codes the same values. Only the BLE build uses them.
#[cfg_attr(not(all(feature = "ble", target_os = "linux")), allow(dead_code))]
pub const SERVICE_UUID: &str = "6f1a0001-8b5e-4c1f-9d3a-1e5f0a7c2b90";
#[cfg_attr(not(all(feature = "ble", target_os = "linux")), allow(dead_code))]
pub const INFO_UUID: &str = "6f1a0002-8b5e-4c1f-9d3a-1e5f0a7c2b90";
#[cfg_attr(not(all(feature = "ble", target_os = "linux")), allow(dead_code))]
pub const RX_UUID: &str = "6f1a0003-8b5e-4c1f-9d3a-1e5f0a7c2b90";
#[cfg_attr(not(all(feature = "ble", target_os = "linux")), allow(dead_code))]
pub const TX_UUID: &str = "6f1a0004-8b5e-4c1f-9d3a-1e5f0a7c2b90";

/// A phone that goes quiet this long is dropped so another can connect.
const LINK_IDLE: Duration = Duration::from_secs(180);
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const SCAN_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(75);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// No Wi-Fi profile at all (first boot).
    Unconfigured,
    /// Offline for `offline_start_mins`.
    Offline,
    /// Long-press on the GPIO button.
    #[cfg_attr(not(all(feature = "hw", target_os = "linux")), allow(dead_code))]
    Button,
    /// `POST /api/v1/provision/start` (debug page, USB).
    Api,
}

/// One end of a phone connection: fragments in, fragments out.
pub struct LinkIo {
    pub rx: mpsc::Receiver<Vec<u8>>,
    pub tx: mpsc::Sender<Vec<u8>>,
    /// Largest fragment the transport can carry right now (ATT MTU − 3).
    pub max_fragment: Arc<AtomicUsize>,
}

struct Run {
    reason: Reason,
    code: String,
    started: Instant,
    last_activity: Instant,
    failures: u32,
    /// The current code is on the panel.
    painted: bool,
    /// A phone said hello, so the code should be painted.
    wants_paint: bool,
    client: bool,
    finished: bool,
}

#[derive(Default)]
struct Inner {
    run: Option<Run>,
    wifi: Option<WifiState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub active: bool,
    pub reason: Option<Reason>,
    pub active_secs: Option<u64>,
    pub client_connected: bool,
    pub code_on_panel: bool,
    pub transport: ProvisionTransport,
    pub wifi: Option<WifiState>,
}

pub struct Provisioner {
    inner: Mutex<Inner>,
    pub wifi: Wifi,
    transport: ProvisionTransport,
    wake: tokio::sync::Notify,
}

impl Provisioner {
    pub fn new(cfg: &ProvisionConfig, wifi_iface: &str) -> Self {
        let wifi = match cfg.wifi {
            common::config::WifiBackend::Nmcli => Wifi::Nmcli {
                iface: wifi_iface.to_string(),
            },
            common::config::WifiBackend::Mock => Wifi::Mock(wifi::MockWifi::new(
                cfg.mock_configured,
                cfg.mock_ip.clone(),
            )),
        };
        Self {
            inner: Mutex::default(),
            wifi,
            transport: cfg.transport,
            wake: tokio::sync::Notify::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts a run (no-op if one is active). Returns whether a new run began.
    pub fn start(&self, reason: Reason) -> bool {
        let mut g = self.lock();
        if g.run.is_some() {
            return false;
        }
        let now = Instant::now();
        g.run = Some(Run {
            reason,
            code: crypto::new_code(),
            started: now,
            last_activity: now,
            failures: 0,
            painted: false,
            wants_paint: reason == Reason::Unconfigured,
            client: false,
            finished: false,
        });
        g.wifi = None;
        drop(g);
        tracing::info!(?reason, "provisioning started");
        self.wake.notify_one();
        true
    }

    pub fn stop(&self) {
        if let Some(r) = self.lock().run.as_mut() {
            r.finished = true;
        }
        self.wake.notify_one();
    }

    pub fn active(&self) -> bool {
        self.lock().run.is_some()
    }

    /// The panel shows the setup code, so the scheduler must not paint over it.
    pub fn holds_display(&self) -> bool {
        self.lock()
            .run
            .as_ref()
            .is_some_and(|r| r.painted || r.wants_paint)
    }

    pub fn status(&self) -> Status {
        let g = self.lock();
        let run = g.run.as_ref();
        Status {
            active: run.is_some(),
            reason: run.map(|r| r.reason),
            active_secs: run.map(|r| r.started.elapsed().as_secs()),
            client_connected: run.is_some_and(|r| r.client),
            code_on_panel: run.is_some_and(|r| r.painted),
            transport: self.transport,
            wifi: g.wifi.clone(),
        }
    }

    /// For the debug endpoint only; the panel is the real channel.
    pub fn current_code(&self) -> Option<String> {
        self.code()
    }

    fn code(&self) -> Option<String> {
        self.lock().run.as_ref().map(|r| r.code.clone())
    }

    fn touch(&self) {
        if let Some(r) = self.lock().run.as_mut() {
            r.last_activity = Instant::now();
        }
    }

    fn set_client(&self, connected: bool) {
        if let Some(r) = self.lock().run.as_mut() {
            r.client = connected;
            r.last_activity = Instant::now();
        }
    }

    fn client_hello(&self) {
        let paint = {
            let mut g = self.lock();
            match g.run.as_mut() {
                Some(r) if !r.painted && !r.wants_paint => {
                    r.wants_paint = true;
                    true
                }
                _ => false,
            }
        };
        if paint {
            self.wake.notify_one();
        }
    }

    /// Counts a wrong code. After `max` failures the code is replaced (and
    /// repainted), which turns online guessing into a 25-second-per-try game.
    fn auth_failed(&self, max: u32) -> Option<String> {
        let mut g = self.lock();
        let r = g.run.as_mut()?;
        r.failures += 1;
        if r.failures < max.max(1) {
            return None;
        }
        tracing::warn!(
            failures = r.failures,
            "too many wrong setup codes; rotating the code"
        );
        r.failures = 0;
        r.code = crypto::new_code();
        r.painted = false;
        r.wants_paint = true;
        let code = r.code.clone();
        drop(g);
        self.wake.notify_one();
        Some(code)
    }

    fn set_wifi(&self, s: WifiState) {
        self.lock().wifi = Some(s);
    }

    fn finish(&self) {
        if let Some(r) = self.lock().run.as_mut() {
            r.finished = true;
        }
        self.wake.notify_one();
    }
}

/// Whether to stop an active run now.
fn should_stop(
    r: &Run,
    cfg: &ProvisionConfig,
    configured: Option<bool>,
    online: bool,
    now: Instant,
) -> bool {
    if r.finished {
        return true;
    }
    if r.client {
        return false;
    }
    match r.reason {
        // Keep waiting until the device has a working network.
        Reason::Unconfigured => configured == Some(true) && online,
        Reason::Offline => online,
        Reason::Button | Reason::Api => {
            now.duration_since(r.last_activity)
                >= Duration::from_secs(cfg.idle_timeout_mins.max(1) * 60)
        }
    }
}

/// Supervised task: starts/stops runs and the transport, paints the code.
pub async fn run(app: Arc<App>) -> anyhow::Result<()> {
    // Aborted on drop, so a restarted task never leaves an old radio session behind.
    let mut transport: Option<AbortOnDrop> = None;
    let mut configured: Option<bool> = None;
    let mut checked_at: Option<Instant> = None;
    loop {
        app.health.checkpoint(TASK).await;
        app.health.beat(TASK);
        let cfg = app.config();
        let pcfg = &cfg.provision;
        let now = Instant::now();

        if checked_at.is_none_or(|t| now.duration_since(t) >= Duration::from_secs(30)) {
            configured = app.provision.wifi.configured().await;
            checked_at = Some(now);
        }
        let (online, offline_for) = {
            let n = app.net.lock().unwrap_or_else(|e| e.into_inner());
            (n.online, n.offline_since.map(|t| now.duration_since(t)))
        };

        if pcfg.enabled && !app.provision.active() {
            if configured == Some(false) {
                app.provision.start(Reason::Unconfigured);
            } else if pcfg.offline_start_mins > 0
                && cfg.network.enabled
                && offline_for
                    .is_some_and(|d| d >= Duration::from_secs(pcfg.offline_start_mins * 60))
            {
                app.provision.start(Reason::Offline);
            }
        }

        // Stop?
        let stop = {
            let g = app.provision.lock();
            g.run.as_ref().map(|r| {
                (
                    should_stop(r, pcfg, configured, online, now),
                    r.painted || r.wants_paint,
                )
            })
        };
        if let Some((true, was_on_panel)) = stop {
            app.provision.lock().run = None;
            transport = None;
            tracing::info!("provisioning stopped");
            checked_at = None;
            if was_on_panel {
                // Put the scheduled content back.
                let _ = app.tx.send(Command::Refresh(Target::Auto)).await;
            }
        }

        if app.provision.active() {
            if transport.as_ref().is_none_or(|t| t.0.is_finished()) {
                transport = Some(AbortOnDrop(start_transport(&app)));
            }
            paint_if_needed(&app).await;
            app.health.ok(TASK);
        } else {
            transport = None;
        }

        tokio::select! {
            _ = app.provision.wake.notified() => {}
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
        }
    }
}

async fn paint_if_needed(app: &Arc<App>) {
    let job = {
        let g = app.provision.lock();
        g.run
            .as_ref()
            .filter(|r| r.wants_paint && !r.painted)
            .map(|r| r.code.clone())
    };
    let Some(code) = job else { return };
    let name = app.identity.name.clone();
    let fonts = app.fonts.clone();
    let frame = tokio::task::spawn_blocking(move || {
        render::setup::render(&fonts, &name, &crypto::display_code(&code))
    })
    .await;
    match frame {
        Ok(frame) => {
            if app
                .tx
                .send(Command::Show(Box::new(frame), "provisioning".into()))
                .await
                .is_ok()
            {
                if let Some(r) = app.provision.lock().run.as_mut() {
                    r.painted = true;
                }
            }
        }
        Err(e) => tracing::error!(error = %e, "could not render the setup screen"),
    }
}

fn start_transport(app: &Arc<App>) -> tokio::task::JoinHandle<()> {
    let (links_tx, mut links_rx) = mpsc::channel::<LinkIo>(1);
    let a = app.clone();
    let info = info_json(app);
    let server = match app.provision.transport {
        ProvisionTransport::Sim => {
            let listen = app.config().provision.sim_listen.clone();
            tokio::spawn(async move {
                if let Err(e) = sim::serve(&listen, info, links_tx).await {
                    a.health.error(TASK, format!("simulator: {e:#}"));
                    tracing::error!(error = %format!("{e:#}"), "provisioning simulator stopped");
                }
            })
        }
        ProvisionTransport::Ble => {
            #[cfg(all(feature = "ble", target_os = "linux"))]
            {
                let name = app.identity.name.clone();
                tokio::spawn(async move {
                    if let Err(e) = ble::serve(&name, info, links_tx).await {
                        a.health.error(TASK, format!("ble: {e:#}"));
                        tracing::error!(error = %format!("{e:#}"), "BLE provisioning stopped");
                    }
                })
            }
            #[cfg(not(all(feature = "ble", target_os = "linux")))]
            {
                let _ = (links_tx, info);
                tokio::spawn(async move {
                    a.health.error(
                        TASK,
                        "built without the `ble` feature; set provision.transport = \"sim\"",
                    );
                    tracing::error!("BLE provisioning requested but this build has no BLE support");
                    std::future::pending::<()>().await;
                })
            }
        }
    };
    let app = app.clone();
    tokio::spawn(async move {
        // Dropping this task (abort) drops the server and with it the radio.
        let _server = AbortOnDrop(server);
        while let Some(link) = links_rx.recv().await {
            app.provision.set_client(true);
            serve_link(&app, link).await;
            app.provision.set_client(false);
        }
    })
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The plaintext "info" characteristic.
fn info_json(app: &App) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "proto": session::PROTO_VERSION,
        "id": app.identity.id,
        "name": app.identity.name,
        "fw": env!("CARGO_PKG_VERSION"),
    }))
    .unwrap_or_default()
}

async fn send(link: &LinkIo, msg: &[u8]) -> bool {
    let max = link.max_fragment.load(Ordering::Relaxed);
    for f in frame::fragment(msg, max) {
        match tokio::time::timeout(SEND_TIMEOUT, link.tx.send(f)).await {
            Ok(Ok(())) => {}
            _ => return false,
        }
    }
    true
}

async fn serve_link(app: &Arc<App>, mut link: LinkIo) {
    let Some(code) = app.provision.code() else {
        return;
    };
    tracing::info!("provisioning client connected");
    let mut session = Session::new(code);
    let mut reasm = frame::Reassembler::default();
    loop {
        let frag = match tokio::time::timeout(LINK_IDLE, link.rx.recv()).await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => {
                tracing::info!("provisioning client idle; dropping it");
                break;
            }
        };
        app.provision.touch();
        let msg = match reasm.push(&frag) {
            Ok(Some(m)) => m,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(error = %e, "bad provisioning fragment");
                continue;
            }
        };
        let mut close = false;
        for ev in session.handle(&msg) {
            match ev {
                Event::Send(b) => close |= !send(&link, &b).await,
                // Someone is trying to pair: show them the code.
                Event::Hello => app.provision.client_hello(),
                Event::Authenticated => tracing::info!("provisioning client authenticated"),
                Event::AuthFailed => {
                    if let Some(code) = app
                        .provision
                        .auth_failed(app.config().provision.max_auth_failures)
                    {
                        session.set_code(code);
                    }
                }
                Event::Close => close = true,
                Event::Request(req) => {
                    close |= !handle_request(app, &mut session, &link, req).await
                }
            }
        }
        if close {
            break;
        }
    }
    tracing::info!("provisioning client disconnected");
}

/// Returns false when the link should close.
async fn handle_request(
    app: &Arc<App>,
    session: &mut Session,
    link: &LinkIo,
    req: Request,
) -> bool {
    let reply = |session: &mut Session, r: Reply| session.seal(&r);
    let out = match req {
        Request::Scan => {
            match tokio::time::timeout(SCAN_TIMEOUT, app.provision.wifi.scan()).await {
                Ok(Ok(list)) => Reply::Networks { list },
                Ok(Err(e)) => Reply::Error {
                    code: session::ErrorCode::BadMessage,
                    msg: format!("scan failed: {e:#}"),
                },
                Err(_) => Reply::Error {
                    code: session::ErrorCode::BadMessage,
                    msg: "scan timed out".into(),
                },
            }
        }
        Request::Connect { ssid, psk, hidden } => {
            tracing::info!(%ssid, hidden, "provisioning: connecting to Wi-Fi");
            let connecting = WifiState::Connecting { ssid: ssid.clone() };
            app.provision.set_wifi(connecting.clone());
            if let Some(b) = reply(session, Reply::State { state: connecting }) {
                if !send(link, &b).await {
                    return false;
                }
            }
            let result = tokio::time::timeout(
                CONNECT_TIMEOUT,
                app.provision.wifi.connect(&ssid, &psk, hidden),
            )
            .await;
            let state = match result {
                Ok(Ok(l)) => {
                    let (host, port) = app.http_endpoint();
                    tracing::info!(%ssid, ip = ?l.ip, "provisioning: Wi-Fi connected");
                    WifiState::Connected {
                        ssid,
                        ip: l.ip.unwrap_or_default(),
                        host,
                        port,
                    }
                }
                Ok(Err(e)) => {
                    tracing::warn!(%ssid, reason = ?e.reason, detail = %e.detail, "provisioning: Wi-Fi failed");
                    WifiState::Failed {
                        ssid,
                        reason: e.reason,
                        detail: e.detail,
                    }
                }
                Err(_) => WifiState::Failed {
                    ssid,
                    reason: wifi::FailReason::Timeout,
                    detail: "connect timed out".into(),
                },
            };
            app.provision.set_wifi(state.clone());
            app.provision.touch();
            Reply::State { state }
        }
        Request::Status => {
            let state = match app.provision.status().wifi {
                Some(s) => s,
                None => {
                    let cur = app.provision.wifi.current().await;
                    WifiState::Idle {
                        ssid: cur.as_ref().map(|l| l.ssid.clone()),
                        ip: cur.and_then(|l| l.ip),
                    }
                }
            };
            Reply::State { state }
        }
        Request::Done => {
            if let Some(b) = reply(session, Reply::Bye) {
                send(link, &b).await;
            }
            app.provision.finish();
            return false;
        }
    };
    match reply(session, out) {
        Some(b) => send(link, &b).await,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with(reason: Reason) -> Run {
        let now = Instant::now();
        Run {
            reason,
            code: "0".repeat(12),
            started: now,
            last_activity: now,
            failures: 0,
            painted: false,
            wants_paint: false,
            client: false,
            finished: false,
        }
    }

    #[test]
    fn stop_rules() {
        let cfg = ProvisionConfig::default();
        let now = Instant::now();
        let later = now + Duration::from_secs(cfg.idle_timeout_mins * 60 + 1);
        let r = run_with(Reason::Unconfigured);
        assert!(!should_stop(&r, &cfg, Some(false), true, later));
        assert!(!should_stop(&r, &cfg, Some(true), false, later));
        assert!(should_stop(&r, &cfg, Some(true), true, now));
        let r = run_with(Reason::Offline);
        assert!(!should_stop(&r, &cfg, Some(true), false, later));
        assert!(should_stop(&r, &cfg, Some(true), true, now));
        let mut r = run_with(Reason::Api);
        assert!(!should_stop(&r, &cfg, Some(true), true, now));
        assert!(should_stop(&r, &cfg, Some(true), true, later));
        // Never cut off a phone mid-conversation.
        r.client = true;
        assert!(!should_stop(&r, &cfg, Some(true), true, later));
        r.finished = true;
        assert!(should_stop(&r, &cfg, Some(true), true, now));
    }

    #[test]
    fn code_rotates_after_repeated_failures() {
        let p = Provisioner::new(&ProvisionConfig::default(), "wlan0");
        assert!(p.start(Reason::Api));
        assert!(!p.start(Reason::Button));
        let first = p.code();
        assert!(p.auth_failed(3).is_none());
        assert!(p.auth_failed(3).is_none());
        let rotated = p.auth_failed(3);
        assert!(rotated.is_some() && rotated != first);
        assert_eq!(p.code(), rotated);
        assert!(p.holds_display());
    }
}
