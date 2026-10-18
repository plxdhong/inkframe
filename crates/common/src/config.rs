//! TOML configuration with a schema version and defaults for every field.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("read config {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("unsupported config version {0}, expected {CONFIG_VERSION}")]
    Version(u32),
    #[error("invalid config: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub timezone: String,
    pub data_dir: PathBuf,
    pub fonts_dir: PathBuf,
    pub display: DisplayConfig,
    pub weather: WeatherConfig,
    pub holiday: HolidayConfig,
    pub schedule: ScheduleConfig,
    pub thresholds: Thresholds,
    pub http: HttpConfig,
    pub network: NetworkConfig,
    pub watchdog: WatchdogConfig,
    pub photo: PhotoConfig,
    pub provision: ProvisionConfig,
    pub mdns: MdnsConfig,
    pub power: PowerConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            timezone: "Asia/Shanghai".into(),
            data_dir: PathBuf::from("/data/inkframe"),
            fonts_dir: PathBuf::from("/opt/inkframe/fonts"),
            display: DisplayConfig::default(),
            weather: WeatherConfig::default(),
            holiday: HolidayConfig::default(),
            schedule: ScheduleConfig::default(),
            thresholds: Thresholds::default(),
            http: HttpConfig::default(),
            network: NetworkConfig::default(),
            watchdog: WatchdogConfig::default(),
            photo: PhotoConfig::default(),
            provision: ProvisionConfig::default(),
            mdns: MdnsConfig::default(),
            power: PowerConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// Real panel over spidev + gpiocdev (Linux only).
    Hardware,
    /// In-memory panel for local development; frames are only kept as previews.
    Mock,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayConfig {
    pub backend: Backend,
    pub spi_device: String,
    pub gpio_chip: String,
    pub pins: Pins,
    pub busy_timeout_secs: u64,
    pub max_retries: u32,
    /// Measured panel colours used for quantisation and previews, as `#RRGGBB`.
    pub palette: PaletteConfig,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            backend: Backend::Hardware,
            spi_device: "/dev/spidev0.0".into(),
            gpio_chip: "/dev/gpiochip0".into(),
            pins: Pins::default(),
            busy_timeout_secs: 60,
            max_retries: 2,
            palette: PaletteConfig::default(),
        }
    }
}

/// BCM pin numbers. Only the newer HAT revision has the PWR (panel supply) pin;
/// set `has_pwr = false` for the older one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Pins {
    pub rst: u32,
    pub dc: u32,
    pub busy: u32,
    pub pwr: u32,
    pub has_pwr: bool,
}

impl Default for Pins {
    fn default() -> Self {
        Self {
            rst: 17,
            dc: 25,
            busy: 24,
            pwr: 18,
            has_pwr: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PaletteConfig {
    pub black: String,
    pub white: String,
    pub yellow: String,
    pub red: String,
    pub blue: String,
    pub green: String,
}

impl Default for PaletteConfig {
    fn default() -> Self {
        Self {
            black: "#1C1F22".into(),
            white: "#EDEBE4".into(),
            yellow: "#E6CC2E".into(),
            red: "#B4161B".into(),
            blue: "#1F56B3".into(),
            green: "#2E6B3F".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WeatherProvider {
    Qweather,
    Mock,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WeatherConfig {
    pub provider: WeatherProvider,
    /// Per-account API host, e.g. `abc123.re.qweatherapi.com`.
    pub api_host: String,
    /// API-key auth. Leave empty to use JWT auth (`jwt_*`) instead.
    pub api_key: String,
    /// JWT auth (QWeather console → project → credential, Ed25519): the
    /// project id (`sub`), the credential id (`kid`) and the PKCS#8 PEM
    /// private key file.
    pub jwt_project_id: String,
    pub jwt_key_id: String,
    pub jwt_private_key_path: String,
    /// QWeather location id (Shenzhen = 101280601).
    pub location: String,
    pub city_name: String,
    pub request_timeout_secs: u64,
    /// Start fetching this many minutes before a dashboard refresh.
    pub prefetch_minutes: i64,
    pub poll_minutes: i64,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            provider: WeatherProvider::Qweather,
            api_host: String::new(),
            api_key: String::new(),
            jwt_project_id: String::new(),
            jwt_key_id: String::new(),
            jwt_private_key_path: String::new(),
            location: "101280601".into(),
            city_name: "深圳".into(),
            request_timeout_secs: 15,
            prefetch_minutes: 10,
            poll_minutes: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct HolidayConfig {
    /// `{year}` is replaced with the year. Empty disables fetching.
    pub url_template: String,
    /// Tried in order when `url_template` fails (GitHub is unreliable from
    /// mainland China without a proxy). Same `{year}` substitution.
    pub mirrors: Vec<String>,
    pub refresh_hours: i64,
}

impl Default for HolidayConfig {
    fn default() -> Self {
        Self {
            url_template:
                "https://raw.githubusercontent.com/NateScarlet/holiday-cn/master/{year}.json".into(),
            mirrors: vec![
                "https://fastly.jsdelivr.net/gh/NateScarlet/holiday-cn@master/{year}.json".into(),
                "https://cdn.jsdelivr.net/gh/NateScarlet/holiday-cn@master/{year}.json".into(),
            ],
            refresh_hours: 24,
        }
    }
}

/// Times are `HH:MM` in the configured timezone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ScheduleConfig {
    pub workday: DaySchedule,
    pub offday: DaySchedule,
    /// Window in which a new weather warning may interrupt and show the dashboard.
    pub alert_window: [String; 2],
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            workday: DaySchedule {
                morning: "07:00".into(),
                photos: vec![
                    "18:00".into(),
                    "19:00".into(),
                    "20:00".into(),
                    "21:00".into(),
                ],
                evening: "22:30".into(),
            },
            offday: DaySchedule {
                morning: "08:00".into(),
                photos: [
                    "10:00", "12:00", "14:00", "16:00", "18:00", "20:00", "22:00",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect(),
                evening: "22:30".into(),
            },
            alert_window: ["07:00".into(), "22:30".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DaySchedule {
    pub morning: String,
    pub photos: Vec<String>,
    pub evening: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Thresholds {
    pub temp_change_c: i32,
    pub rain_probability_pct: u32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            temp_change_c: 5,
            rain_probability_pct: 40,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    /// Bound on all interfaces by default so both wlan0 and the USB gadget reach it.
    pub listen: String,
    pub max_upload_bytes: usize,
    /// Enables /api/v1/debug/* (clock offset, task stall, fake warnings). Development only.
    pub debug_endpoints: bool,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            listen: "0.0.0.0:8080".into(),
            max_upload_bytes: 8 * 1024 * 1024,
            debug_endpoints: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkConfig {
    pub enabled: bool,
    /// Only log the recovery commands instead of running them.
    pub dry_run: bool,
    pub check_interval_secs: u64,
    /// TCP targets probed for connectivity; any success counts as online.
    pub probes: Vec<String>,
    pub wifi_interface: String,
    pub usb_interface: String,
    /// Consecutive failed checks before each escalation step. The ladder
    /// repeats every `reboot_after` checks while the outage lasts; reconnects
    /// also repeat every `reconnect_after`. Each reboot that did not bring the
    /// network back doubles the next reboot threshold (up to 16x).
    pub reconnect_after: u32,
    pub restart_service_after: u32,
    /// Unload and reload the Wi-Fi driver (the Pi's brcmfmac SDIO firmware can
    /// wedge with the interface still "connected"). 0 disables.
    pub reload_driver_after: u32,
    pub reboot_after: u32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dry_run: false,
            check_interval_secs: 60,
            probes: vec!["223.5.5.5:53".into(), "119.29.29.29:53".into()],
            wifi_interface: "wlan0".into(),
            usb_interface: "usb0".into(),
            reconnect_after: 3,
            restart_service_after: 6,
            reload_driver_after: 10,
            reboot_after: 15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WatchdogConfig {
    pub notify_interval_secs: u64,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            notify_interval_secs: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PhotoConfig {
    pub saturation: f32,
    pub contrast: f32,
    pub brightness: f32,
    pub favorite_weight: u32,
    /// Uploads larger than this in either dimension are rejected.
    pub max_dimension: u32,
}

impl Default for PhotoConfig {
    fn default() -> Self {
        Self {
            saturation: 1.4,
            contrast: 1.15,
            brightness: 1.05,
            favorite_weight: 3,
            max_dimension: 2400,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProvisionTransport {
    /// BlueZ GATT peripheral (needs the `ble` build feature, Linux only).
    Ble,
    /// The same protocol over TCP at `sim_listen`, for developing the app without a radio.
    Sim,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WifiBackend {
    /// NetworkManager via `nmcli`.
    Nmcli,
    /// Simulated networks; the password for all of them is `inkframe123`.
    Mock,
}

/// BLE Wi-Fi provisioning (docs/PROVISIONING.md).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ProvisionConfig {
    pub enabled: bool,
    pub transport: ProvisionTransport,
    pub sim_listen: String,
    pub wifi: WifiBackend,
    /// Mock backend only: whether Wi-Fi starts configured, and the address it
    /// reports after connecting (10.0.2.2 is the host as seen from the Android emulator).
    pub mock_configured: bool,
    pub mock_ip: String,
    /// Start advertising after this long offline; 0 disables. The code is only
    /// drawn on the panel once a phone connects.
    pub offline_start_mins: u64,
    /// Runs started by the button or API end after this long without a phone.
    pub idle_timeout_mins: u64,
    /// Wrong setup codes before a new code is drawn.
    pub max_auth_failures: u32,
    /// BCM pin of an optional push button to ground; holding it starts provisioning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button_pin: Option<u32>,
    pub button_hold_secs: u64,
}

impl Default for ProvisionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            transport: ProvisionTransport::Ble,
            sim_listen: "127.0.0.1:8090".into(),
            wifi: WifiBackend::Nmcli,
            mock_configured: true,
            mock_ip: "10.0.2.2".into(),
            offline_start_mins: 30,
            idle_timeout_mins: 10,
            max_auth_failures: 5,
            button_pin: None,
            button_hold_secs: 5,
        }
    }
}

/// Advertises `_inkframe._tcp` so the app finds the device on the LAN.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MdnsConfig {
    pub enabled: bool,
}

impl Default for MdnsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Remote shutdown / reboot over `POST /api/v1/power`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PowerConfig {
    pub enabled: bool,
    /// Paint the power-off screen but only log `systemctl poweroff|reboot`.
    pub dry_run: bool,
}

impl Default for PowerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dry_run: false,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| ConfigError::Io(path.to_owned(), e))?;
        Self::from_toml(&text)
    }

    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let cfg: Config = toml::from_str(text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn to_toml(&self) -> Result<String, ConfigError> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Writes atomically (temp file + rename) so a power cut never leaves half a config.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = self.to_toml()?;
        crate::fsutil::write_atomic(path, text.as_bytes())
            .map_err(|e| ConfigError::Io(path.to_owned(), e))
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.version != CONFIG_VERSION {
            return Err(ConfigError::Version(self.version));
        }
        for day in [&self.schedule.workday, &self.schedule.offday] {
            for t in day.photos.iter().chain([&day.morning, &day.evening]) {
                crate::time::parse_hhmm(t)
                    .ok_or_else(|| ConfigError::Invalid(format!("bad time {t:?}")))?;
            }
        }
        for t in &self.schedule.alert_window {
            crate::time::parse_hhmm(t)
                .ok_or_else(|| ConfigError::Invalid(format!("bad time {t:?}")))?;
        }
        if self.timezone.parse::<chrono_tz::Tz>().is_err() {
            return Err(ConfigError::Invalid(format!(
                "unknown timezone {:?}",
                self.timezone
            )));
        }
        if self.display.busy_timeout_secs == 0 {
            return Err(ConfigError::Invalid("busy_timeout_secs must be > 0".into()));
        }
        Ok(())
    }

    pub fn tz(&self) -> chrono_tz::Tz {
        self.timezone.parse().unwrap_or(chrono_tz::Asia::Shanghai)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        let cfg = Config::from_toml("").ok();
        assert_eq!(cfg, Some(Config::default()));
    }

    #[test]
    fn roundtrip() {
        let cfg = Config::default();
        let text = cfg.to_toml().ok();
        let back = text.and_then(|t| Config::from_toml(&t).ok());
        assert_eq!(back, Some(cfg));
    }

    #[test]
    fn rejects_wrong_version_and_bad_time() {
        assert!(matches!(
            Config::from_toml("version = 2"),
            Err(ConfigError::Version(2))
        ));
        assert!(Config::from_toml(
            "[schedule.workday]\nmorning = \"7am\"\nphotos=[]\nevening=\"22:30\""
        )
        .is_err());
    }
}
