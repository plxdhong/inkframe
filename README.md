# inkframe — 墨水屏相册

A photo frame and calendar/weather dashboard for a Raspberry Pi Zero 2W with a Waveshare 7.3"
e-Paper HAT (E) (E Ink Spectra 6, six colours, 800×480). Written in Rust, with a Kotlin Android app
for setup and photos. The first goal is to run for months without anyone touching it.

树莓派 Zero 2W + 微雪 7.3 寸六色墨水屏的相册：按时间表显示日历天气仪表盘和照片轮播，配套安卓 App
负责蓝牙配网、传照片和设置。设计目标是放着几个月不用管。

## What it does

- **Dashboard** in the morning and evening: date, lunar calendar and solar terms, Chinese statutory
  holidays and make-up workdays, weather from [QWeather](https://dev.qweather.com/) (和风天气),
  and hints when today differs from yesterday (temperature swing, umbrella). Weather warnings
  interrupt the schedule.
- **Photos** during the day, shuffled so a round never repeats. Photos are enhanced and dithered in
  Lab space to the panel's six inks; the app shows the dithered result before you push it.
- **Phone app**: BLE Wi-Fi provisioning (X25519 + setup code shown on the panel + AES-GCM), LAN
  discovery over mDNS, status, photo upload (5:3 crop), and settings.
- **Unattended operation**: scheduling recomputed from the wall clock, a supervisor that restarts
  failed tasks, a systemd watchdog, a network recovery ladder (reconnect → restart NetworkManager →
  reload the Wi-Fi driver → reboot with back-off), atomic writes, and an optional read-only root.
- **Debug page** and HTTP API on port 8080, also reachable over the USB gadget port (`10.12.0.1`).

## Try it without hardware

Everything is mocked by `config/dev.toml` (panel, weather, Wi-Fi, provisioning over TCP):

```bash
cargo run -p inkframe -- --config config/dev.toml          # open http://127.0.0.1:8080
cargo run -p render --example preview -- target/preview    # dashboard and test-pattern PNGs
cargo test --workspace && cargo clippy --workspace --all-targets
```

The Android app builds with only Docker:

```bash
app/android/tools/docker-gradle.sh testDebugUnitTest assembleDebug
```

## On a device

See [docs/DEVICE.md](docs/DEVICE.md) for flashing, the data partition, install
(`deploy/deploy.sh --setup`), bring-up checks and daily operation. Weather needs your own QWeather
credentials in `/data/inkframe/config.toml`: either `weather.api_key`, or `jwt_project_id` +
`jwt_key_id` + `jwt_private_key_path` (Ed25519). All other settings have defaults; see
[config/inkframe.example.toml](config/inkframe.example.toml).

| Path | What |
|---|---|
| `crates/common` | config, frame format, weather types, lunar calendar and holidays |
| `crates/epd` | panel driver (spidev + gpiocdev behind feature `hw`) |
| `crates/render` | drawing, fonts, dashboard layout, photo dithering |
| `crates/daemon` | the `inkframe` service: scheduler, watchdog, HTTP API, BLE provisioning, mDNS |
| `app/android` | Kotlin + Jetpack Compose app |
| `docs/PROVISIONING.md` | BLE provisioning protocol |

## Status

v0.1.1 runs on the author's frame and has been checked end to end with a real phone (Android 15),
including remote reboot and shutdown from the app.
Not verified yet: the static musl build of the BLE feature, Android 10/11, and long-term soak
results. The default colour palette is the design palette; calibrate it for your panel with the
dither test pattern.

## Licence and credits

MIT, see [LICENSE](LICENSE).

- Fonts in `assets/fonts` are Noto Sans SC and Noto Serif SC under the SIL Open Font License
  ([LICENSE-OFL.txt](assets/fonts/LICENSE-OFL.txt)).
- The panel's init sequence follows Waveshare's `EPD_7in3e` reference driver.
- Holiday data is fetched at runtime from [NateScarlet/holiday-cn](https://github.com/NateScarlet/holiday-cn);
  lunar dates come from [tyme4rs](https://crates.io/crates/tyme4rs).
- Builds with the `ble` feature statically include libdbus (dual-licensed AFL-2.1 / GPL-2.0-or-later).
