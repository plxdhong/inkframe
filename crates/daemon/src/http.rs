//! HTTP API v1, shared by the debug page and the phone app.
//!
//! | Method | Path                         | Purpose                                   |
//! |--------|------------------------------|-------------------------------------------|
//! | GET    | /api/v1/status               | health, schedule, panel and data status   |
//! | POST   | /api/v1/refresh              | `{"target":"auto|morning|evening|photo"}` |
//! | GET/PUT| /api/v1/mode                 | `{"mode":"auto|dashboard|photo"}`         |
//! | GET/PUT| /api/v1/config               | full config as JSON                       |
//! | GET    | /api/v1/preview.png          | what the panel currently shows            |
//! | GET    | /api/v1/render/{view}.png    | render morning/evening without refreshing |
//! | GET    | /api/v1/logs?n=200           | recent log lines                          |
//! | POST   | /api/v1/test-pattern         | `{"kind":"colors|dither|white"}`          |
//! | GET    | /api/v1/photos               | list                                      |
//! | POST   | /api/v1/photos               | raw image body (pre-scaled)               |
//! | PATCH  | /api/v1/photos/{id}          | `{"favorite":bool,"paused":bool}`         |
//! | DELETE | /api/v1/photos/{id}          |                                           |
//! | GET    | /api/v1/photos/{id}/source.png | stored 800x480 source                   |
//! | POST   | /api/v1/photos/{id}/show     | put this photo on the panel now           |
//! | POST   | /api/v1/photos/preview       | raw image body → dithered PNG, not stored |
//! | POST   | /api/v1/photos/reprocess     | re-dither all after palette changes       |
//! | GET    | /api/v1/provision            | BLE provisioning state                    |
//! | POST   | /api/v1/provision/start|stop | start/stop provisioning (code on panel)   |
//! | POST   | /api/v1/debug/*              | clock offset, stall, fake warning (opt-in)|
//! | GET    | /api/v1/debug/provision-code | current setup code, for test automation   |

use crate::app::App;
use crate::engine::{self, Command, Target};
use crate::schedule;
use crate::state::Mode;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use common::weather::Warning;
use common::Config;
use render::dashboard::View;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type AppState = Arc<App>;

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

type ApiResult<T> = Result<T, ApiError>;

pub fn router(app: AppState) -> Router {
    let limit = app.config().http.max_upload_bytes;
    let mut api = Router::new()
        .route("/status", get(status))
        .route("/refresh", post(refresh))
        .route("/mode", get(get_mode).put(set_mode))
        .route("/config", get(get_config).put(put_config))
        .route("/preview.png", get(preview))
        .route("/render/{view}", get(render_view))
        .route("/logs", get(logs))
        .route("/test-pattern", post(test_pattern))
        .route("/photos", get(list_photos).post(upload_photo))
        .route("/photos/reprocess", post(reprocess_photos))
        .route("/photos/preview", post(preview_photo))
        .route("/photos/{id}/show", post(show_photo))
        .route("/provision", get(provision_status))
        .route("/provision/start", post(provision_start))
        .route("/provision/stop", post(provision_stop))
        .route("/photos/{id}", patch(update_photo).delete(delete_photo))
        .route("/photos/{id}/source.png", get(photo_source));
    if app.config().http.debug_endpoints {
        api = api
            .route("/debug/clock", post(debug_clock))
            .route("/debug/stall", post(debug_stall))
            .route("/debug/warning", post(debug_warning))
            .route("/debug/provision-code", get(debug_provision_code));
    }
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../assets/index.html")) }),
        )
        .nest("/api/v1", api)
        .layer(DefaultBodyLimit::max(limit))
        .with_state(app)
}

async fn send(app: &App, cmd: Command) -> ApiResult<()> {
    app.tx
        .send(cmd)
        .await
        .map_err(|_| internal("engine is not running"))
}

async fn status(State(app): State<AppState>) -> Json<Value> {
    let cfg = app.config();
    let now = app.clock.local();
    let cal = app.holidays();
    let current = schedule::current(&cfg.schedule, &cal, now);
    let next = schedule::next(&cfg.schedule, &cal, now);
    let st = app.state().clone();
    let snap = app.weather.latest();
    let net = app.net.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "device": app.identity,
        "provision": app.provision.status(),
        "now": now.format("%Y-%m-%d %H:%M:%S").to_string(),
        "clock_offset_secs": app.clock.offset(),
        "uptime_secs": (chrono::Utc::now() - app.started).num_seconds(),
        "workday": cal.is_workday(now.date()),
        "mode": st.mode,
        "current_event": current.map(|e| json!({"at": e.at, "slot": e.slot, "key": e.key()})),
        "next_event": next.map(|e| json!({"at": e.at, "slot": e.slot})),
        "handled_event": st.handled_event,
        "shown": st.shown,
        "display": app.display.status(),
        "tasks": app.health.statuses(),
        "healthy": app.health.unhealthy().is_empty(),
        "weather": snap.as_ref().map(|s| json!({
            "fetched_at": s.fetched_at,
            "days": s.daily.len(),
            "warnings": s.warnings,
            "provider": format!("{:?}", cfg.weather.provider).to_lowercase(),
        })),
        "network": net,
        "photos": app.photos.list().len(),
    }))
}

#[derive(Deserialize)]
struct RefreshReq {
    #[serde(default = "auto")]
    target: Target,
}

fn auto() -> Target {
    Target::Auto
}

async fn refresh(
    State(app): State<AppState>,
    body: Option<Json<RefreshReq>>,
) -> ApiResult<Json<Value>> {
    let target = body.map(|b| b.0.target).unwrap_or(Target::Auto);
    send(&app, Command::Refresh(target)).await?;
    Ok(Json(json!({ "queued": true })))
}

async fn get_mode(State(app): State<AppState>) -> Json<Value> {
    Json(json!({ "mode": app.state().mode }))
}

#[derive(Deserialize)]
struct ModeReq {
    mode: Mode,
}

async fn set_mode(State(app): State<AppState>, Json(req): Json<ModeReq>) -> ApiResult<Json<Value>> {
    app.update_state(|s| {
        s.mode = req.mode;
        // Make the current event re-run under the new mode.
        s.handled_event = None;
    });
    send(&app, Command::Wake).await?;
    Ok(Json(json!({ "mode": req.mode })))
}

async fn get_config(State(app): State<AppState>) -> Json<Config> {
    Json((*app.config()).clone())
}

async fn put_config(
    State(app): State<AppState>,
    Json(cfg): Json<Config>,
) -> ApiResult<Json<Value>> {
    let restart = app.update_config(cfg).map_err(bad)?;
    send(&app, Command::Wake).await?;
    Ok(Json(json!({ "saved": true, "restart_required": restart })))
}

fn png(bytes: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response()
}

async fn preview(State(app): State<AppState>) -> ApiResult<Response> {
    let bytes = app
        .display
        .preview()
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "nothing shown yet".into()))?;
    Ok(png((*bytes).clone()))
}

async fn render_view(State(app): State<AppState>, Path(view): Path<String>) -> ApiResult<Response> {
    let view = match view.trim_end_matches(".png") {
        "morning" => View::Morning,
        "evening" => View::Evening,
        other => return Err(bad(format!("unknown view {other:?}"))),
    };
    let model = engine::dashboard_model(&app, view, app.clock.local());
    let fonts = app.fonts.clone();
    let palette = app.palette();
    let bytes = tokio::task::spawn_blocking(move || {
        render::preview_png(&render::dashboard::render(&model, &fonts), &palette)
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    Ok(png(bytes))
}

#[derive(Deserialize)]
struct LogsQuery {
    n: Option<usize>,
}

async fn logs(State(app): State<AppState>, Query(q): Query<LogsQuery>) -> Json<Vec<String>> {
    Json(app.logs.tail(q.n.unwrap_or(200).min(500)))
}

#[derive(Deserialize)]
struct PatternReq {
    kind: String,
}

async fn test_pattern(
    State(app): State<AppState>,
    Json(req): Json<PatternReq>,
) -> ApiResult<Json<Value>> {
    let fonts = app.fonts.clone();
    let palette = app.palette();
    let photo_cfg = app.config().photo.clone();
    let kind = req.kind.clone();
    let frame = tokio::task::spawn_blocking(move || match kind.as_str() {
        "colors" => Some(render::patterns::color_bars(&fonts)),
        "dither" => Some(render::patterns::dither_sample(&palette)),
        "dither-enhanced" => Some(render::patterns::dither_sample_enhanced(
            &palette, &photo_cfg,
        )),
        "white" => Some(common::Frame::filled(common::Ink::White)),
        _ => None,
    })
    .await
    .map_err(internal)?
    .ok_or_else(|| bad(format!("unknown pattern {:?}", req.kind)))?;
    send(
        &app,
        Command::Show(Box::new(frame), format!("pattern:{}", req.kind)),
    )
    .await?;
    Ok(Json(json!({ "queued": true })))
}

async fn list_photos(State(app): State<AppState>) -> Json<Value> {
    Json(json!(app.photos.list()))
}

async fn upload_photo(State(app): State<AppState>, body: Bytes) -> ApiResult<Json<Value>> {
    if body.is_empty() {
        return Err(bad("empty body"));
    }
    let cfg = app.config().photo.clone();
    let palette = app.palette();
    let a = app.clone();
    let meta = tokio::task::spawn_blocking(move || a.photos.add(&body, &cfg, &palette))
        .await
        .map_err(internal)?
        .map_err(|e| bad(format!("{e:#}")))?;
    tracing::info!(id = %meta.id, "photo added");
    Ok(Json(json!(meta)))
}

#[derive(Deserialize)]
struct PhotoPatch {
    favorite: Option<bool>,
    paused: Option<bool>,
}

async fn update_photo(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(p): Json<PhotoPatch>,
) -> ApiResult<Json<Value>> {
    let meta = app
        .photos
        .update(&id, p.favorite, p.paused)
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, format!("{e:#}")))?;
    Ok(Json(json!(meta)))
}

async fn delete_photo(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    app.photos
        .delete(&id)
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, format!("{e:#}")))?;
    app.update_state(|s| s.photo_deck.retain(|d| d != &id));
    Ok(Json(json!({ "deleted": id })))
}

async fn photo_source(State(app): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let bytes = app
        .photos
        .source_png(&id)
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, format!("{e:#}")))?;
    Ok(png(bytes))
}

/// Runs the full photo pipeline on an upload and returns how the panel would
/// show it, without storing anything (the app's "preview before pushing").
async fn preview_photo(State(app): State<AppState>, body: Bytes) -> ApiResult<Response> {
    if body.is_empty() {
        return Err(bad("empty body"));
    }
    let cfg = app.config().photo.clone();
    let palette = app.palette();
    let bytes = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let img = render::photo::decode_upload(&body, cfg.max_dimension)?;
        let (_, frame) = render::photo::process(&img, &cfg, &palette);
        Ok(render::preview_png(&frame, &palette)?)
    })
    .await
    .map_err(internal)?
    .map_err(|e| bad(format!("{e:#}")))?;
    Ok(png(bytes))
}

async fn show_photo(State(app): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let frame = app
        .photos
        .frame(&id)
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, format!("{e:#}")))?;
    send(&app, Command::Show(Box::new(frame), format!("photo:{id}"))).await?;
    Ok(Json(json!({ "queued": true })))
}

async fn provision_status(State(app): State<AppState>) -> Json<Value> {
    Json(json!(app.provision.status()))
}

async fn provision_start(State(app): State<AppState>) -> ApiResult<Json<Value>> {
    if !app.config().provision.enabled {
        return Err(bad("provisioning is disabled (provision.enabled = false)"));
    }
    let started = app.provision.start(crate::provision::Reason::Api);
    Ok(Json(
        json!({ "started": started, "status": app.provision.status() }),
    ))
}

async fn provision_stop(State(app): State<AppState>) -> Json<Value> {
    app.provision.stop();
    Json(json!({ "stopping": true }))
}

async fn reprocess_photos(State(app): State<AppState>) -> ApiResult<Json<Value>> {
    let cfg = app.config().photo.clone();
    let palette = app.palette();
    let a = app.clone();
    let n = tokio::task::spawn_blocking(move || a.photos.reprocess_all(&cfg, &palette))
        .await
        .map_err(internal)?
        .map_err(|e| internal(format!("{e:#}")))?;
    Ok(Json(json!({ "reprocessed": n })))
}

#[derive(Deserialize)]
struct ClockReq {
    /// Absolute local time "YYYY-MM-DD HH:MM", or relative seconds.
    at: Option<String>,
    offset_secs: Option<i64>,
}

async fn debug_clock(
    State(app): State<AppState>,
    Json(req): Json<ClockReq>,
) -> ApiResult<Json<Value>> {
    let offset = match (req.at, req.offset_secs) {
        (Some(at), _) => {
            let target =
                chrono::NaiveDateTime::parse_from_str(&at, "%Y-%m-%d %H:%M").map_err(bad)?;
            let real =
                (app.clock.now() - chrono::Duration::seconds(app.clock.offset())).naive_local();
            (target - real).num_seconds()
        }
        (None, Some(s)) => s,
        (None, None) => 0,
    };
    app.clock.set_offset(offset);
    tracing::warn!(offset_secs = offset, now = %app.clock.local(), "debug: clock offset changed");
    send(&app, Command::Wake).await?;
    Ok(Json(
        json!({ "offset_secs": offset, "now": app.clock.local() }),
    ))
}

#[derive(Deserialize)]
struct StallReq {
    task: String,
}

async fn debug_stall(State(app): State<AppState>, Json(req): Json<StallReq>) -> Json<Value> {
    tracing::warn!(task = %req.task, "debug: stalling task to exercise the watchdog");
    app.health.stall(&req.task);
    if req.task == engine::TASK {
        // The engine only reaches its checkpoint when woken.
        let _ = app.tx.send(Command::Wake).await;
    }
    Json(json!({ "stalled": req.task }))
}

/// The code normally only appears on the panel; exposing it is acceptable only
/// with debug endpoints on (development), so app tests can run unattended.
async fn debug_provision_code(State(app): State<AppState>) -> ApiResult<Json<Value>> {
    let code = app
        .provision
        .current_code()
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "provisioning is not active".into()))?;
    Ok(Json(json!({ "code": code })))
}

#[derive(Deserialize)]
struct WarningReq {
    type_name: String,
    color: String,
}

async fn debug_warning(
    State(app): State<AppState>,
    Json(req): Json<WarningReq>,
) -> ApiResult<Json<Value>> {
    let mock = app
        .mock_weather
        .as_ref()
        .ok_or_else(|| bad("only available with the mock weather provider"))?;
    let w = Warning {
        id: format!("mock-{}", chrono::Utc::now().timestamp()),
        title: format!("{}{}预警", req.type_name, req.color),
        type_name: req.type_name,
        color: req.color,
        pub_time: app.clock.local().format("%H:%M").to_string(),
    };
    mock.warnings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(w.clone());
    // Fetch now so the weather task sees it on its next pass.
    let snap = app
        .provider
        .fetch(app.clock.now().fixed_offset())
        .await
        .map_err(internal)?;
    app.weather
        .store(snap, app.clock.local().date())
        .map_err(internal)?;
    let now = app.clock.local();
    if schedule::in_alert_window(&app.config().schedule, now) {
        send(&app, Command::Alert(vec![w.clone()])).await?;
    }
    Ok(Json(json!(w)))
}
