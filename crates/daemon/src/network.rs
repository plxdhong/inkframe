//! Connectivity self-healing in escalating steps: reconnect Wi-Fi, restart
//! NetworkManager, reload the Wi-Fi driver, then reboot — but never reboot
//! while the USB debug link is up. The ladder repeats for as long as the
//! outage lasts, and reboots back off so a long router outage does not turn
//! into a reboot every quarter hour.

use common::config::NetworkConfig;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    None,
    Reconnect,
    RestartService,
    ReloadDriver,
    Reboot,
    /// Reboot was due but the USB gadget link is connected.
    RebootSkipped,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct NetStatus {
    pub online: bool,
    pub consecutive_failures: u32,
    pub last_action: Option<Action>,
    /// Start of the current outage (for starting provisioning).
    #[serde(skip)]
    pub offline_since: Option<std::time::Instant>,
}

/// Reboot threshold after `net_reboots` reboots that did not help: doubles
/// each time, capped at 16x (15 min → 4 h with the defaults).
pub fn reboot_threshold(cfg: &NetworkConfig, net_reboots: u32) -> u32 {
    cfg.reboot_after
        .max(1)
        .saturating_mul(1 << net_reboots.min(4))
}

/// Decides the recovery step for the current failure count.
pub fn decide(cfg: &NetworkConfig, failures: u32, net_reboots: u32, usb_link_up: bool) -> Action {
    if failures == 0 {
        return Action::None;
    }
    let reboot = reboot_threshold(cfg, net_reboots);
    if failures.is_multiple_of(reboot) {
        return if usb_link_up {
            Action::RebootSkipped
        } else {
            Action::Reboot
        };
    }
    // Position inside the current round of the ladder.
    let phase = failures % cfg.reboot_after.max(1);
    if cfg.restart_service_after > 0 && phase == cfg.restart_service_after {
        Action::RestartService
    } else if cfg.reload_driver_after > 0 && phase == cfg.reload_driver_after {
        Action::ReloadDriver
    } else if cfg.reconnect_after > 0 && phase.is_multiple_of(cfg.reconnect_after) {
        Action::Reconnect
    } else {
        Action::None
    }
}

pub async fn probe(targets: &[String]) -> bool {
    for t in targets {
        let ok =
            tokio::time::timeout(Duration::from_secs(3), tokio::net::TcpStream::connect(t)).await;
        if matches!(ok, Ok(Ok(_))) {
            return true;
        }
    }
    false
}

pub fn usb_link_up(iface: &str) -> bool {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/carrier"))
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

pub async fn run_action(cfg: &NetworkConfig, action: Action) {
    let cmds: &[&[&str]] = match action {
        Action::Reconnect => &[
            &["nmcli", "device", "disconnect", &cfg.wifi_interface],
            &["nmcli", "device", "connect", &cfg.wifi_interface],
        ],
        Action::RestartService => &[&["systemctl", "restart", "NetworkManager"]],
        // brcmfmac_wcc holds a reference on brcmfmac, so it goes first.
        Action::ReloadDriver => &[
            &["modprobe", "-r", "brcmfmac_wcc", "brcmfmac"],
            &["modprobe", "brcmfmac"],
        ],
        Action::Reboot => &[&["systemctl", "reboot"]],
        Action::None | Action::RebootSkipped => &[],
    };
    for argv in cmds {
        if cfg.dry_run {
            tracing::warn!(cmd = %argv.join(" "), "network recovery (dry run)");
            continue;
        }
        tracing::warn!(cmd = %argv.join(" "), "network recovery");
        if let Err(e) = run(argv).await {
            tracing::error!(error = %format!("{e:#}"), "recovery command failed");
        }
    }
}

/// Runs a command with a 30 s cap and returns its stdout.
async fn run(argv: &[&str]) -> anyhow::Result<String> {
    let (prog, args) = argv
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty command"))?;
    let out = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(prog)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("timed out: {}", argv.join(" ")))??;
    anyhow::ensure!(
        out.status.success(),
        "{}: {} {}",
        argv.join(" "),
        out.status,
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether an `802-11-wireless.powersave` value leaves power saving on.
/// NetworkManager prints `default`/`ignore`/`disable`/`enable` or the number.
pub fn powersave_on(value: &str) -> bool {
    !matches!(value.trim(), "disable" | "2")
}

/// Turns Wi-Fi power saving off on every saved Wi-Fi profile. A profile left at
/// `default` falls back to the driver default (on), which is what took the old
/// build off the network: the profile its BLE flow created kept the default.
/// Returns whether the active profile was changed (it only takes effect on the
/// next activation).
pub async fn enforce_powersave_off(cfg: &NetworkConfig) -> bool {
    if cfg.dry_run {
        return false;
    }
    let list = match run(&[
        "nmcli",
        "-t",
        "-f",
        "UUID,TYPE,DEVICE",
        "connection",
        "show",
    ])
    .await
    {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "cannot list Wi-Fi profiles");
            return false;
        }
    };
    let mut active_changed = false;
    for line in list.lines() {
        let mut f = line.split(':');
        let (Some(uuid), Some(kind), device) = (f.next(), f.next(), f.next().unwrap_or_default())
        else {
            continue;
        };
        if kind != "802-11-wireless" {
            continue;
        }
        let current = run(&[
            "nmcli",
            "-g",
            "802-11-wireless.powersave",
            "connection",
            "show",
            uuid,
        ])
        .await
        .unwrap_or_default();
        if !powersave_on(&current) {
            continue;
        }
        match run(&[
            "nmcli",
            "connection",
            "modify",
            uuid,
            "802-11-wireless.powersave",
            "2",
        ])
        .await
        {
            Ok(_) => {
                tracing::warn!(
                    uuid,
                    was = current.trim(),
                    "turned Wi-Fi power saving off on profile"
                );
                active_changed |= device == cfg.wifi_interface;
            }
            Err(e) => {
                tracing::error!(uuid, error = %format!("{e:#}"), "cannot turn Wi-Fi power saving off")
            }
        }
    }
    active_changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ladder(cfg: &NetworkConfig, net_reboots: u32, n: u32) -> Vec<(u32, Action)> {
        (0..=n)
            .map(|i| (i, decide(cfg, i, net_reboots, false)))
            .filter(|(_, a)| *a != Action::None)
            .collect()
    }

    #[test]
    fn escalation_ladder() {
        let cfg = NetworkConfig::default(); // 3 / 6 / 10 / 15
        use Action::*;
        assert_eq!(
            ladder(&cfg, 0, 15),
            vec![
                (3, Reconnect),
                (6, RestartService),
                (9, Reconnect),
                (10, ReloadDriver),
                (12, Reconnect),
                (15, Reboot)
            ]
        );
        assert_eq!(decide(&cfg, 15, 0, true), RebootSkipped);
        // With the USB link up the ladder keeps going instead of going quiet.
        assert_eq!(decide(&cfg, 18, 0, true), Reconnect);
        assert_eq!(decide(&cfg, 21, 0, true), RestartService);
        assert_eq!(decide(&cfg, 30, 0, true), RebootSkipped);
    }

    #[test]
    fn reboots_back_off() {
        let cfg = NetworkConfig::default();
        assert_eq!(reboot_threshold(&cfg, 0), 15);
        assert_eq!(reboot_threshold(&cfg, 1), 30);
        assert_eq!(reboot_threshold(&cfg, 9), 240);
        // After one unhelpful reboot, 15 is just another round of the ladder.
        assert_eq!(decide(&cfg, 15, 1, false), Action::Reconnect);
        assert_eq!(decide(&cfg, 25, 1, false), Action::ReloadDriver);
        assert_eq!(decide(&cfg, 30, 1, false), Action::Reboot);
    }

    #[test]
    fn zero_thresholds_do_not_divide_by_zero() {
        let cfg = NetworkConfig {
            reconnect_after: 0,
            restart_service_after: 0,
            reload_driver_after: 0,
            reboot_after: 0,
            ..NetworkConfig::default()
        };
        assert_eq!(decide(&cfg, 1, 0, false), Action::Reboot);
    }

    #[test]
    fn powersave_values() {
        assert!(powersave_on("default"));
        assert!(powersave_on("0"));
        assert!(powersave_on("enable\n"));
        assert!(!powersave_on("disable\n"));
        assert!(!powersave_on("2"));
    }
}
