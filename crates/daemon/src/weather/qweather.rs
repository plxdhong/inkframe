//! QWeather (和风天气) client. Responses carry numbers as strings.
//! Docs: https://dev.qweather.com/docs/api/

use anyhow::{bail, Context};
use chrono::{DateTime, FixedOffset, NaiveDate};
use common::config::WeatherConfig;
use common::weather::{DailyForecast, Warning, WeatherSnapshot};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

pub struct Client {
    http: reqwest::Client,
    base: String,
    auth: Auth,
    location: String,
}

enum Auth {
    Key(String),
    Jwt {
        project: String,
        kid: String,
        key: Box<ed25519_dalek::SigningKey>,
    },
}

impl Auth {
    fn from_config(cfg: &WeatherConfig) -> anyhow::Result<Self> {
        if !cfg.api_key.is_empty() {
            return Ok(Auth::Key(cfg.api_key.clone()));
        }
        if cfg.jwt_project_id.is_empty()
            || cfg.jwt_key_id.is_empty()
            || cfg.jwt_private_key_path.is_empty()
        {
            bail!("set weather.api_key, or weather.jwt_project_id + jwt_key_id + jwt_private_key_path");
        }
        let pem = std::fs::read_to_string(&cfg.jwt_private_key_path)
            .with_context(|| format!("read {}", cfg.jwt_private_key_path))?;
        let key =
            <ed25519_dalek::SigningKey as ed25519_dalek::pkcs8::DecodePrivateKey>::from_pkcs8_pem(
                &pem,
            )
            .map_err(|e| anyhow::anyhow!("parse Ed25519 key {}: {e}", cfg.jwt_private_key_path))?;
        Ok(Auth::Jwt {
            project: cfg.jwt_project_id.clone(),
            kid: cfg.jwt_key_id.clone(),
            key: Box::new(key),
        })
    }

    /// (header name, value) for one request.
    fn header(&self, now_unix: i64) -> (&'static str, String) {
        match self {
            Auth::Key(k) => ("X-QW-Api-Key", k.clone()),
            Auth::Jwt { project, kid, key } => (
                "Authorization",
                format!("Bearer {}", jwt(project, kid, key, now_unix)),
            ),
        }
    }
}

/// EdDSA JWT as QWeather expects: `iat` 30 s in the past for clock skew,
/// valid for 15 minutes.
fn jwt(project: &str, kid: &str, key: &ed25519_dalek::SigningKey, now_unix: i64) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
    use base64::Engine;
    use ed25519_dalek::Signer;
    let iat = now_unix - 30;
    let header = serde_json::json!({ "alg": "EdDSA", "kid": kid });
    let claims = serde_json::json!({ "sub": project, "iat": iat, "exp": iat + 900 });
    let signing_input = format!(
        "{}.{}",
        B64.encode(header.to_string()),
        B64.encode(claims.to_string())
    );
    let sig = key.sign(signing_input.as_bytes());
    format!("{signing_input}.{}", B64.encode(sig.to_bytes()))
}

#[derive(Deserialize)]
struct Envelope<T> {
    code: String,
    #[serde(flatten)]
    body: Option<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Daily {
    fx_date: NaiveDate,
    temp_max: String,
    temp_min: String,
    icon_day: String,
    text_day: String,
    text_night: String,
    wind_dir_day: String,
    wind_scale_day: String,
    humidity: String,
    precip: String,
}

#[derive(Deserialize)]
struct DailyBody {
    daily: Vec<Daily>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hourly {
    #[serde(deserialize_with = "qw_time")]
    fx_time: DateTime<FixedOffset>,
    pop: Option<String>,
}

/// QWeather times look like `2026-10-17T15:00+08:00` (no seconds), which
/// RFC 3339 parsing rejects.
fn qw_time<'de, D: serde::Deserializer<'de>>(d: D) -> Result<DateTime<FixedOffset>, D::Error> {
    let s = String::deserialize(d)?;
    DateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M%:z")
        .or_else(|_| DateTime::parse_from_rfc3339(&s))
        .map_err(serde::de::Error::custom)
}

#[derive(Deserialize)]
struct HourlyBody {
    hourly: Vec<Hourly>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiWarning {
    id: String,
    title: String,
    type_name: String,
    severity_color: Option<String>,
    #[serde(deserialize_with = "qw_time")]
    pub_time: DateTime<FixedOffset>,
}

#[derive(Deserialize)]
struct WarningBody {
    warning: Vec<ApiWarning>,
}

#[derive(Deserialize)]
struct AirNow {
    category: String,
}

#[derive(Deserialize)]
struct AirBody {
    now: AirNow,
}

fn num<T: std::str::FromStr + Default>(s: &str) -> T {
    s.trim()
        .parse::<f64>()
        .ok()
        .and_then(|v| v.round().to_string().parse().ok())
        .unwrap_or_default()
}

impl Client {
    pub fn new(cfg: &WeatherConfig) -> anyhow::Result<Self> {
        if cfg.api_host.is_empty() {
            bail!("weather.api_host must be set for the qweather provider");
        }
        let auth = Auth::from_config(cfg)?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(cfg.request_timeout_secs))
            .connect_timeout(Duration::from_secs(cfg.request_timeout_secs.min(10)))
            .gzip(true)
            .build()?;
        let host = cfg.api_host.trim_end_matches('/');
        let base = if host.starts_with("http") {
            host.to_string()
        } else {
            format!("https://{host}")
        };
        Ok(Self {
            http,
            base,
            auth,
            location: cfg.location.clone(),
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let url = format!("{}{}?location={}", self.base, path, self.location);
        let (auth_name, auth_value) = self.auth.header(chrono::Utc::now().timestamp());
        let resp = self
            .http
            .get(&url)
            .header(auth_name, auth_value)
            .send()
            .await
            .with_context(|| format!("GET {path}"))?;
        let status = resp.status();
        let env: Envelope<T> = resp
            .json()
            .await
            .with_context(|| format!("decode {path} (HTTP {status})"))?;
        if env.code != "200" {
            bail!("{path}: api code {}", env.code);
        }
        env.body.with_context(|| format!("{path}: empty body"))
    }

    pub async fn fetch(&self, now: DateTime<FixedOffset>) -> anyhow::Result<WeatherSnapshot> {
        let (daily, hourly, warnings, air) = tokio::join!(
            self.get::<DailyBody>("/v7/weather/7d"),
            self.get::<HourlyBody>("/v7/weather/24h"),
            self.get::<WarningBody>("/v7/warning/now"),
            self.get::<AirBody>("/v7/air/now"),
        );
        let daily = daily?;
        // Max hourly precipitation probability per date, daytime hours only.
        let mut pop: HashMap<NaiveDate, u32> = HashMap::new();
        match hourly {
            Ok(h) => {
                for hr in h.hourly {
                    let t = hr.fx_time;
                    if !(7..=22).contains(&chrono::Timelike::hour(&t)) {
                        continue;
                    }
                    if let Some(p) = hr.pop.as_deref().map(num::<u32>) {
                        let e = pop.entry(t.date_naive()).or_default();
                        *e = (*e).max(p);
                    }
                }
            }
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "hourly forecast unavailable"),
        }
        let daily = daily
            .daily
            .into_iter()
            .map(|d| DailyForecast {
                date: d.fx_date,
                temp_max: num(&d.temp_max),
                temp_min: num(&d.temp_min),
                text_day: d.text_day,
                text_night: d.text_night,
                icon_day: d.icon_day,
                precip_prob: pop.get(&d.fx_date).copied(),
                precip_mm: d.precip.trim().parse().unwrap_or(0.0),
                humidity: num(&d.humidity),
                wind_dir: d.wind_dir_day,
                wind_scale: d.wind_scale_day,
            })
            .collect();
        let warnings = match warnings {
            Ok(w) => w
                .warning
                .into_iter()
                .map(|w| Warning {
                    id: w.id,
                    title: w.title,
                    type_name: w.type_name,
                    color: w.severity_color.unwrap_or_default(),
                    pub_time: w.pub_time.format("%H:%M").to_string(),
                })
                .collect(),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "warnings unavailable");
                Vec::new()
            }
        };
        let air = air.ok().map(|a| a.now.category);
        Ok(WeatherSnapshot {
            fetched_at: now,
            daily,
            air,
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hourly_time_without_seconds() {
        let body: HourlyBody =
            serde_json::from_str(r#"{"hourly":[{"fxTime":"2026-10-17T15:00+08:00","pop":"20"}]}"#)
                .map_err(|e| e.to_string())
                .unwrap_or(HourlyBody { hourly: vec![] });
        assert_eq!(body.hourly.len(), 1);
        assert_eq!(
            body.hourly
                .first()
                .map(|h| chrono::Timelike::hour(&h.fx_time)),
            Some(15)
        );
    }

    #[test]
    fn jwt_verifies_with_the_public_key() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
        use base64::Engine;
        use ed25519_dalek::Verifier;
        let key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let token = jwt("proj", "kid1", &key, 1_800_000_000);
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let claims: serde_json::Value =
            serde_json::from_slice(&B64.decode(parts[1]).unwrap_or_default()).unwrap_or_default();
        assert_eq!(claims["sub"], "proj");
        assert_eq!(
            claims["exp"].as_i64().unwrap_or_default() - claims["iat"].as_i64().unwrap_or_default(),
            900
        );
        let sig_bytes: [u8; 64] = B64
            .decode(parts[2])
            .unwrap_or_default()
            .try_into()
            .unwrap_or([0; 64]);
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
        assert!(key
            .verifying_key()
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
            .is_ok());
    }

    #[test]
    fn parses_daily_payload() -> anyhow::Result<()> {
        let json = r#"{"code":"200","updateTime":"2026-10-15T05:00+08:00","daily":[{"fxDate":"2026-10-15",
            "tempMax":"26","tempMin":"21","iconDay":"306","textDay":"中雨","textNight":"阴",
            "windDirDay":"东北风","windScaleDay":"1-3","humidity":"85","precip":"12.5"}]}"#;
        let env: Envelope<DailyBody> = serde_json::from_str(json)?;
        assert_eq!(env.code, "200");
        let body = env.body.ok_or_else(|| anyhow::anyhow!("body"))?;
        assert_eq!(num::<i32>(&body.daily[0].temp_max), 26);
        Ok(())
    }

    #[test]
    fn error_code_has_no_body() -> anyhow::Result<()> {
        let env: Envelope<DailyBody> = serde_json::from_str(r#"{"code":"401"}"#)?;
        assert!(env.body.is_none());
        Ok(())
    }
}
