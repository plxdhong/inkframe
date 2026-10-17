# inkframe — 墨水屏相册

Rust daemon for a Raspberry Pi Zero 2W + Waveshare 7.3" e-Paper HAT (E) (Spectra 6, 800×480).
Shows a calendar/weather dashboard and rotates photos on a timetable. Priority #1 is running for
months unattended.

## Layout

| Crate | Role |
|---|---|
| `crates/common` | Config (TOML, versioned, defaults), `Ink`/`Frame` (4-bit packing), weather types, lunar/solar terms/holidays (`tyme4rs` + holiday-cn) |
| `crates/epd` | Panel driver over a `Hal` trait. `MockHal` for dev/tests; `LinuxHal` (spidev + gpiocdev) behind feature `hw`, Linux only |
| `crates/render` | Canvas (no anti-aliasing), fonts (`assets/fonts`, Noto Sans/Serif SC), dashboard layout, Lab Floyd–Steinberg photo dithering, test patterns |
| `crates/daemon` (bin `inkframe`) | Scheduler engine, supervisor + systemd watchdog, display thread, weather/holiday/network tasks, HTTP API + debug page, BLE provisioning (`provision/`), mDNS (`mdns.rs`) |
| `app/android` | Kotlin + Jetpack Compose phone app (P3): BLE provisioning, LAN discovery, status/photos/settings over the HTTP API. See `app/android/README.md` |

## Run locally (everything mocked)

```bash
cargo run -p inkframe -- --config config/dev.toml     # http://127.0.0.1:8080
cargo run -p render --example preview -- target/preview   # dashboard + pattern PNGs
cargo test --workspace && cargo clippy --workspace --all-targets
```

`config/dev.toml`: mock panel, mock weather (matches the design mockup), network recovery dry-run,
debug endpoints on, provisioning over the TCP simulator (`:8090`) with mock Wi-Fi (password `inkframe123`). Debug API: `POST /api/v1/debug/clock {"at":"2026-10-15 06:59"}` moves time,
`/debug/warning` injects a weather warning, `/debug/stall {"task":"weather"}` hangs a task to test the watchdog.

## Reliability rules (hard requirements, enforced in review)

- No `unwrap`/`expect`/`panic!` — denied by workspace clippy lints (tests use `unwrap_or_default` / `?`).
- Every IO has a timeout (HTTP clients, BUSY wait, recovery commands, display jobs).
- Scheduling is recomputed from the wall clock every ≤60 s (`schedule::current` vs persisted
  `handled_event`), never by summing sleeps. Missed events, reboots and clock jumps self-correct;
  no image stays >24 h.
- Each task heartbeats into `Health`; `supervise` restarts tasks that error/panic; the watchdog only
  pets systemd while all tasks are within their silence budget.
- Writes are atomic (`fsutil::write_atomic`). Config, state, photos, the binary and fonts live on
  `/data` so the root can be a read-only overlay.

## Android app

```bash
app/android/tools/docker-gradle.sh testDebugUnitTest assembleDebug   # needs only Docker
```

`ProtocolTest.kt` replays `crates/daemon/src/provision/testdata/vector.txt`. If the protocol or
crypto changes, regenerate it (`cargo test -p inkframe cross_language_vector -- --nocapture`) and
keep both sides passing.

## Provisioning

Spec: `docs/PROVISIONING.md` (GATT layout, framing, X25519 + setup-code HKDF + AES-GCM).
A run starts on no Wi-Fi profile, after `offline_start_mins` offline, on a GPIO long-press, or via
`POST /api/v1/provision/start`. While it is active the engine does not paint over the setup code
and the network task skips recovery. `transport = "ble"` needs feature `ble` (bluer, vendored
libdbus).

## HTTP API v1

See the table at the top of `crates/daemon/src/http.rs`. Shared by the debug page (`assets/index.html`,
embedded) and the Kotlin app.

## Config

All keys optional; `config/inkframe.example.toml` is generated from defaults
(`cargo run -p common --example default_config`). Regenerate it when adding fields.

## Device

Cross-build + deploy: `deploy/deploy.sh` (`cross` → musl, else Docker `rust:1-bookworm` → glibc;
features `hw,ble`): `PI=<ssh host> [PROXY=http://127.0.0.1:<port>] ./deploy/deploy.sh`. The
proxy is only for build/apt (the Pi gets it via `ssh -R`); the device itself must work without one.
First-time setup, data-partition notes and the Wi-Fi drop post-mortem: `docs/DEVICE.md`.

## Status

- v0.1.0 (tag `v0.1.0`) is in daily use since 2026-10-17 and the 30-day soak (P4) runs alongside:
  `PI=<ssh host> ./deploy/soak-check.sh` once or twice a day, see `docs/DEVICE.md` §6. Release bundles go
  to `dist/` (ignored) via `deploy/package.sh`.
- Running on the real device (Debian 13, glibc build with `hw,ble`) since 2026-10-17:
  QWeather via JWT, holidays, network ladder (driver reload tested live), persistent journald.
  Panel refreshes take ~14 s and the dashboard was visually confirmed on the panel (six colours and
  layout correct) on 2026-10-17.
- The default palette is the design palette, not measured panel colours; calibrate with the dither
  test pattern.
- P2/P3 verified on hardware on 2026-10-17 with a Redmi Note 12 Turbo (Android 15): mDNS discovery,
  status/photos/settings over LAN, upload → dithered preview (~1.5 s, 4-bit indexed PNG; the Pi's
  Wi-Fi only gives the phone ~150–200 KB/s) → show/rotate/favourite/pause/delete, and BLE
  provisioning end to end through real BlueZ (scan, GATT, setup code on panel, join, powersave off,
  schedule redrawn). The musl cross-build of vendored libdbus is still unverified.
- Driving the phone: `adb` (Homebrew `android-platform-tools`). MIUI needs 开发者选项 → USB 安装 and
  USB 调试（安全设置）, and asks on the phone for each fresh install; `adb install -r` over an
  existing install does not, as long as the debug keystore volume (`inkframe-android-home`) is kept.
