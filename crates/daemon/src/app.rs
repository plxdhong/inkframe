//! Shared application state handed to every task and HTTP handler.

use crate::clock::Clock;
use crate::display::Display;
use crate::engine::Command;
use crate::health::Health;
use crate::logs::LogBuffer;
use crate::network::NetStatus;
use crate::photos::PhotoStore;
use crate::provision::Provisioner;
use crate::state::{State, StateFile};
use crate::weather::mock::MockWeather;
use crate::weather::{qweather, Provider, WeatherStore};
use anyhow::Context;
use chrono::{DateTime, Utc};
use common::calendar::HolidayCalendar;
use common::config::WeatherProvider;
use common::Config;
use render::{Fonts, Palette};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use tokio::sync::mpsc;

pub struct App {
    pub cfg_path: PathBuf,
    cfg: RwLock<Arc<Config>>,
    pub clock: Clock,
    pub fonts: Arc<Fonts>,
    palette: RwLock<Palette>,
    pub health: Health,
    pub logs: LogBuffer,
    state: Mutex<State>,
    state_file: StateFile,
    pub weather: WeatherStore,
    pub provider: Provider,
    pub mock_weather: Option<MockWeather>,
    holidays: RwLock<Arc<HolidayCalendar>>,
    pub photos: PhotoStore,
    pub display: Display,
    pub tx: mpsc::Sender<Command>,
    pub net: Mutex<NetStatus>,
    pub http: reqwest::Client,
    pub started: DateTime<Utc>,
    pub identity: crate::device::Identity,
    pub provision: Provisioner,
}

impl App {
    pub fn new(
        cfg: Config,
        cfg_path: PathBuf,
        logs: LogBuffer,
        tx: mpsc::Sender<Command>,
    ) -> anyhow::Result<Arc<Self>> {
        std::fs::create_dir_all(&cfg.data_dir)
            .with_context(|| format!("create data dir {}", cfg.data_dir.display()))?;
        let fonts = Fonts::load(&cfg.fonts_dir)
            .with_context(|| format!("load fonts from {}", cfg.fonts_dir.display()))?;
        let palette = Palette::from_config(&cfg.display.palette);
        let health = Health::default();
        let display = Display::start(
            &cfg.display,
            health.clone(),
            palette.clone(),
            cfg.data_dir.join("last_frame.bin"),
        )?;
        let (provider, mock_weather) = match cfg.weather.provider {
            WeatherProvider::Mock => {
                let m = MockWeather::default();
                (Provider::Mock(m.clone()), Some(m))
            }
            WeatherProvider::Qweather => match qweather::Client::new(&cfg.weather) {
                Ok(c) => (Provider::QWeather(Box::new(c)), None),
                // A startup error here would crash-loop the service into
                // StartLimitAction=reboot; run without weather instead.
                Err(e) => {
                    tracing::error!(error = %format!("{e:#}"), "weather provider unavailable; dashboards show cached data");
                    (Provider::Unavailable(format!("{e:#}")), None)
                }
            },
        };
        let state_file = StateFile::new(&cfg.data_dir);
        let state = state_file.load();
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()?;
        let identity = crate::device::Identity::load(&cfg.data_dir);
        let provision = Provisioner::new(&cfg.provision, &cfg.network.wifi_interface);
        let app = Self {
            identity,
            provision,
            clock: Clock::new(cfg.tz()),
            weather: WeatherStore::open(&cfg.data_dir),
            photos: PhotoStore::new(&cfg.data_dir),
            holidays: RwLock::new(Arc::new(crate::tasks::load_cached_holidays(&cfg.data_dir))),
            fonts: Arc::new(fonts),
            palette: RwLock::new(palette),
            cfg: RwLock::new(Arc::new(cfg)),
            cfg_path,
            health,
            logs,
            state: Mutex::new(state),
            state_file,
            provider,
            mock_weather,
            display,
            tx,
            net: Mutex::new(NetStatus {
                online: true,
                ..Default::default()
            }),
            http,
            started: Utc::now(),
        };
        Ok(Arc::new(app))
    }

    pub fn config(&self) -> Arc<Config> {
        self.cfg.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Validates, persists and applies a new configuration. Returns whether a
    /// restart is needed for parts that are only read at startup.
    pub fn update_config(&self, new: Config) -> anyhow::Result<bool> {
        new.validate()?;
        let old = self.config();
        new.save(&self.cfg_path)?;
        let restart = old.display.backend != new.display.backend
            || old.display.pins != new.display.pins
            || old.display.spi_device != new.display.spi_device
            || old.display.busy_timeout_secs != new.display.busy_timeout_secs
            || old.http.listen != new.http.listen
            || old.weather != new.weather
            || old.data_dir != new.data_dir
            || old.fonts_dir != new.fonts_dir
            || old.timezone != new.timezone
            || old.provision.transport != new.provision.transport
            || old.provision.wifi != new.provision.wifi
            || old.provision.sim_listen != new.provision.sim_listen
            || old.provision.button_pin != new.provision.button_pin
            || old.mdns != new.mdns;
        *self.palette.write().unwrap_or_else(|e| e.into_inner()) =
            Palette::from_config(&new.display.palette);
        *self.cfg.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        Ok(restart)
    }

    /// mDNS host name and HTTP port the phone should use after provisioning.
    pub fn http_endpoint(&self) -> (String, u16) {
        let port = self
            .config()
            .http
            .listen
            .rsplit(':')
            .next()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);
        (format!("{}.local", self.identity.name), port)
    }

    pub fn palette(&self) -> Palette {
        self.palette
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn holidays(&self) -> Arc<HolidayCalendar> {
        self.holidays
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_holidays(&self, cal: HolidayCalendar) {
        *self.holidays.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(cal);
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Applies `f` to the state and persists it.
    pub fn update_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let mut g = self.state();
        let r = f(&mut g);
        if let Err(e) = self.state_file.save(&g) {
            tracing::error!(error = %format!("{e:#}"), "could not persist state");
        }
        r
    }
}
