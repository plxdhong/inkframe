//! Wi-Fi operations needed by provisioning, over NetworkManager's `nmcli`
//! (every call has a timeout) or an in-memory mock for development.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Network {
    pub ssid: String,
    /// 0–100.
    pub signal: u8,
    pub secure: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailReason {
    AuthFailed,
    NotFound,
    Timeout,
    Error,
}

/// What the phone is told about the device's Wi-Fi.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WifiState {
    Idle {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ssid: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ip: Option<String>,
    },
    Connecting {
        ssid: String,
    },
    Connected {
        ssid: String,
        ip: String,
        /// mDNS name and HTTP port, so the app can switch to the HTTP API.
        host: String,
        port: u16,
    },
    Failed {
        ssid: String,
        reason: FailReason,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub ssid: String,
    pub ip: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectError {
    pub reason: FailReason,
    pub detail: String,
}

#[derive(Clone)]
pub enum Wifi {
    Nmcli { iface: String },
    Mock(MockWifi),
}

const CMD_TIMEOUT: Duration = Duration::from_secs(20);
/// nmcli's own activation wait; the process timeout sits above it.
const CONNECT_WAIT_SECS: u64 = 45;
/// Our profiles are named `inkframe-<ssid>`, so we only ever replace our own.
const PROFILE_PREFIX: &str = "inkframe-";

impl Wifi {
    pub async fn scan(&self) -> anyhow::Result<Vec<Network>> {
        match self {
            Wifi::Mock(m) => Ok(m.networks()),
            Wifi::Nmcli { iface } => {
                let out = nmcli(
                    &[
                        "-t",
                        "-f",
                        "SSID,SIGNAL,SECURITY",
                        "device",
                        "wifi",
                        "list",
                        "ifname",
                        iface,
                        "--rescan",
                        "yes",
                    ],
                    CMD_TIMEOUT,
                )
                .await?;
                anyhow::ensure!(out.ok, "nmcli scan failed: {}", out.stderr.trim());
                Ok(parse_scan(&out.stdout))
            }
        }
    }

    /// Whether any Wi-Fi profile exists. `None` when that cannot be determined
    /// (then provisioning is not started automatically).
    pub async fn configured(&self) -> Option<bool> {
        match self {
            Wifi::Mock(m) => Some(m.lock().configured),
            Wifi::Nmcli { .. } => {
                let out = nmcli(&["-t", "-f", "TYPE", "connection", "show"], CMD_TIMEOUT)
                    .await
                    .ok()
                    .filter(|o| o.ok)?;
                Some(out.stdout.lines().any(|l| l.trim() == "802-11-wireless"))
            }
        }
    }

    pub async fn current(&self) -> Option<Link> {
        match self {
            Wifi::Mock(m) => m.lock().link.clone(),
            Wifi::Nmcli { iface } => {
                let out = nmcli(
                    &[
                        "-t",
                        "-f",
                        "ACTIVE,SSID",
                        "device",
                        "wifi",
                        "list",
                        "ifname",
                        iface,
                        "--rescan",
                        "no",
                    ],
                    CMD_TIMEOUT,
                )
                .await
                .ok()?;
                let ssid = parse_active_ssid(&out.stdout)?;
                Some(Link {
                    ssid,
                    ip: ipv4(iface).await,
                })
            }
        }
    }

    pub async fn connect(&self, ssid: &str, psk: &str, hidden: bool) -> Result<Link, ConnectError> {
        match self {
            Wifi::Mock(m) => m.connect(ssid, psk, hidden).await,
            Wifi::Nmcli { iface } => nmcli_connect(iface, ssid, psk, hidden).await,
        }
    }
}

async fn nmcli_connect(
    iface: &str,
    ssid: &str,
    psk: &str,
    hidden: bool,
) -> Result<Link, ConnectError> {
    let other = |detail: String| ConnectError {
        reason: FailReason::Error,
        detail,
    };
    let profile = format!("{PROFILE_PREFIX}{ssid}");
    // A previous attempt for the same network (maybe with an old password).
    let _ = nmcli(&["connection", "delete", "id", &profile], CMD_TIMEOUT).await;
    let wait = CONNECT_WAIT_SECS.to_string();
    let mut argv = vec!["--wait", &wait, "device", "wifi", "connect", ssid];
    if !psk.is_empty() {
        argv.extend(["password", psk]);
    }
    argv.extend(["ifname", iface, "name", &profile]);
    if hidden {
        argv.extend(["hidden", "yes"]);
    }
    let out = nmcli(&argv, Duration::from_secs(CONNECT_WAIT_SECS + 15))
        .await
        .map_err(|e| other(format!("{e:#}")))?;
    if !out.ok {
        // Never keep a profile that does not work: NM would retry it forever.
        let _ = nmcli(&["connection", "delete", "id", &profile], CMD_TIMEOUT).await;
        return Err(classify_failure(out.code, &out.stderr));
    }
    // Prefer the network the user just chose over older profiles, and never
    // leave power saving at the driver default (on): that is what dropped the
    // old build off Wi-Fi after its BLE flow created a profile like this one.
    let _ = nmcli(
        &[
            "connection",
            "modify",
            "id",
            &profile,
            "connection.autoconnect-priority",
            "10",
            "802-11-wireless.powersave",
            "2",
        ],
        CMD_TIMEOUT,
    )
    .await;
    Ok(Link {
        ssid: ssid.to_string(),
        ip: ipv4(iface).await,
    })
}

async fn ipv4(iface: &str) -> Option<String> {
    let out = nmcli(&["-g", "IP4.ADDRESS", "device", "show", iface], CMD_TIMEOUT)
        .await
        .ok()?;
    parse_ipv4(&out.stdout)
}

struct Output {
    ok: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

async fn nmcli(argv: &[&str], timeout: Duration) -> anyhow::Result<Output> {
    tracing::debug!(cmd = %redact(argv), "nmcli");
    let fut = tokio::process::Command::new("nmcli")
        .args(argv)
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| anyhow::anyhow!("nmcli timed out: {}", redact(argv)))??;
    Ok(Output {
        ok: out.status.success(),
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// The command line with the Wi-Fi password masked, for logs.
fn redact(argv: &[&str]) -> String {
    let mut out = Vec::with_capacity(argv.len());
    let mut hide = false;
    for a in argv {
        out.push(if hide { "***" } else { a });
        hide = *a == "password";
    }
    format!("nmcli {}", out.join(" "))
}

/// Splits an `nmcli -t` line on unescaped colons and unescapes `\:` / `\\`.
fn split_terse(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let (Some(n), Some(f)) = (chars.next(), fields.last_mut()) {
                    f.push(n);
                }
            }
            ':' => fields.push(String::new()),
            c => {
                if let Some(f) = fields.last_mut() {
                    f.push(c);
                }
            }
        }
    }
    fields
}

/// Strongest entry per SSID, hidden networks dropped, strongest first, capped
/// so the reply fits one protocol message.
pub fn parse_scan(stdout: &str) -> Vec<Network> {
    let mut out: Vec<Network> = Vec::new();
    for line in stdout.lines() {
        let f = split_terse(line);
        let [ssid, signal, security] = f.as_slice() else {
            continue;
        };
        if ssid.is_empty() {
            continue;
        }
        let signal = signal.trim().parse::<u8>().unwrap_or(0).min(100);
        let secure = !security.trim().is_empty() && security.trim() != "--";
        match out.iter_mut().find(|n| &n.ssid == ssid) {
            Some(n) if n.signal >= signal => {}
            Some(n) => {
                *n = Network {
                    ssid: ssid.clone(),
                    signal,
                    secure,
                }
            }
            None => out.push(Network {
                ssid: ssid.clone(),
                signal,
                secure,
            }),
        }
    }
    out.sort_by_key(|n| std::cmp::Reverse(n.signal));
    out.truncate(25);
    out
}

fn parse_active_ssid(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .map(split_terse)
        .find(|f| f.first().map(String::as_str) == Some("yes"))
        .and_then(|f| f.get(1).cloned())
}

fn parse_ipv4(stdout: &str) -> Option<String> {
    let first = stdout.lines().next()?.split(" | ").next()?.trim();
    let ip = first.split('/').next()?.trim();
    (!ip.is_empty()).then(|| ip.to_string())
}

/// Maps nmcli's exit code and message to something a phone can explain.
/// Exit codes: 3 = timeout, 4 = activation failed, 10 = object not found.
pub fn classify_failure(code: Option<i32>, stderr: &str) -> ConnectError {
    let s = stderr.to_ascii_lowercase();
    let reason = if s.contains("secrets were required")
        || s.contains("no secrets")
        || s.contains("psk")
        || s.contains("password")
    {
        FailReason::AuthFailed
    } else if s.contains("no network with ssid") || code == Some(10) {
        FailReason::NotFound
    } else if s.contains("timeout") || s.contains("timed out") || code == Some(3) {
        FailReason::Timeout
    } else {
        FailReason::Error
    };
    let detail: String = stderr.trim().chars().take(200).collect();
    ConnectError { reason, detail }
}

/// Simulated radio for development. The networks' password is `inkframe123`.
#[derive(Clone)]
pub struct MockWifi {
    inner: Arc<Mutex<MockState>>,
    ip: String,
}

pub struct MockState {
    pub configured: bool,
    pub link: Option<Link>,
}

const MOCK_PSK: &str = "inkframe123";

impl MockWifi {
    pub fn new(configured: bool, ip: String) -> Self {
        let link = configured.then(|| Link {
            ssid: "Home-5G".into(),
            ip: Some(ip.clone()),
        });
        Self {
            inner: Arc::new(Mutex::new(MockState { configured, link })),
            ip,
        }
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, MockState> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn networks(&self) -> Vec<Network> {
        [
            ("Home-5G", 82, true),
            ("Home", 64, true),
            ("Guest", 48, false),
            ("邻居家的WiFi", 21, true),
        ]
        .into_iter()
        .map(|(s, signal, secure)| Network {
            ssid: s.into(),
            signal,
            secure,
        })
        .collect()
    }

    async fn connect(&self, ssid: &str, psk: &str, hidden: bool) -> Result<Link, ConnectError> {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let Some(net) = self.networks().into_iter().find(|n| n.ssid == ssid) else {
            let reason = if hidden {
                FailReason::Timeout
            } else {
                FailReason::NotFound
            };
            return Err(ConnectError {
                reason,
                detail: format!("no network with SSID '{ssid}'"),
            });
        };
        if net.secure && psk != MOCK_PSK {
            return Err(ConnectError {
                reason: FailReason::AuthFailed,
                detail: "Secrets were required, but not provided".into(),
            });
        }
        let link = Link {
            ssid: ssid.into(),
            ip: Some(self.ip.clone()),
        };
        let mut st = self.lock();
        st.configured = true;
        st.link = Some(link.clone());
        Ok(link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_parsing() {
        let out = "Home:70:WPA2\nHome:82:WPA2\n:90:WPA2\nCafe\\: free:40:\nOld:30:--\n";
        let nets = parse_scan(out);
        assert_eq!(
            nets,
            vec![
                Network {
                    ssid: "Home".into(),
                    signal: 82,
                    secure: true
                },
                Network {
                    ssid: "Cafe: free".into(),
                    signal: 40,
                    secure: false
                },
                Network {
                    ssid: "Old".into(),
                    signal: 30,
                    secure: false
                },
            ]
        );
    }

    #[test]
    fn active_and_ip() {
        assert_eq!(
            parse_active_ssid("no:Other\nyes:My\\:Net\n").as_deref(),
            Some("My:Net")
        );
        assert_eq!(parse_active_ssid("no:Other\n"), None);
        assert_eq!(
            parse_ipv4("192.168.1.23/24 | 10.0.0.2/8\n").as_deref(),
            Some("192.168.1.23")
        );
        assert_eq!(parse_ipv4("\n"), None);
    }

    #[test]
    fn failure_classes() {
        let auth =
            "Error: Connection activation failed: (7) Secrets were required, but not provided.";
        assert_eq!(
            classify_failure(Some(4), auth).reason,
            FailReason::AuthFailed
        );
        assert_eq!(
            classify_failure(Some(10), "Error: No network with SSID 'x' found.").reason,
            FailReason::NotFound
        );
        assert_eq!(
            classify_failure(Some(3), "Error: Timeout expired (45 seconds)").reason,
            FailReason::Timeout
        );
        assert_eq!(
            classify_failure(Some(4), "Error: something").reason,
            FailReason::Error
        );
    }

    #[test]
    fn password_redacted() {
        let s = redact(&[
            "device", "wifi", "connect", "Home", "password", "hunter2", "ifname", "wlan0",
        ]);
        assert!(!s.contains("hunter2") && s.contains("***") && s.contains("wlan0"));
    }

    #[tokio::test(start_paused = true)]
    async fn mock_connects_with_the_right_password() {
        let w = Wifi::Mock(MockWifi::new(false, "10.0.2.2".into()));
        assert_eq!(w.configured().await, Some(false));
        assert_eq!(
            w.connect("Home", "nope", false).await.map_err(|e| e.reason),
            Err(FailReason::AuthFailed)
        );
        assert_eq!(
            w.connect("Nowhere", "", false).await.map_err(|e| e.reason),
            Err(FailReason::NotFound)
        );
        assert!(w.connect("Home", MOCK_PSK, false).await.is_ok());
        assert_eq!(w.configured().await, Some(true));
        assert_eq!(w.current().await.map(|l| l.ssid).as_deref(), Some("Home"));
    }
}
