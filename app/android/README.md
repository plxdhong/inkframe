# InkFrame Android app (P3)

Kotlin + Jetpack Compose (Material 3). AGP 9.4 with built-in Kotlin 2.4, Gradle 9.8,
compileSdk/targetSdk 37, minSdk 29.

| Package | Role |
|---|---|
| `provision/` | BLE provisioning protocol (docs/PROVISIONING.md): framing, X25519/HKDF/AES-GCM (BouncyCastle lightweight API), `ProvisioningClient`, `BleTransport` (GATT + coroutines), `SimTransport` (daemon's TCP simulator), `scanFrames` |
| `data/` | `InkFrameApi` (Ktor, HTTP API v1), `DeviceStore` (DataStore), `discoverFrames` (NsdManager, `_inkframe._tcp`), `SettingsForm` (edits known fields inside the full config JSON, so unknown fields survive), `CropState` |
| `ui/` | Devices → Provision (scan, code, Wi-Fi, password, join) → Frame (status / photos / settings tabs) → AddPhoto (5:3 pan-zoom crop → dithered preview from the frame → push) |

## Build and test without Android Studio

Only Docker is needed. The SDK and Gradle caches live in named volumes.

```bash
app/android/tools/docker-gradle.sh testDebugUnitTest assembleDebug
# → app/android/app/build/outputs/apk/debug/app-debug.apk
```

Unit tests run on the JVM. `ProtocolTest` replays the Rust-generated transcript
(`crates/daemon/src/provision/testdata/vector.txt`), so a crypto or encoding mismatch between the
phone and the device fails here first.

End-to-end against a real daemon (no radio):

```bash
# terminal 1 — repo root; dev.toml uses the TCP simulator and mock Wi-Fi
cargo run -p inkframe -- --config config/dev.toml        # http 127.0.0.1:8080, sim 0.0.0.0:8090
# terminal 2 — the container must reach the HTTP port too, so set [http] listen = "0.0.0.0:8080" first
INKFRAME_SIM=host.docker.internal:8090 INKFRAME_API=http://host.docker.internal:8080 \
  app/android/tools/docker-gradle.sh :app:testDebugUnitTest --rerun
```

## In the emulator

Debug builds show a "开发：模拟器" card on the add-frame screen. It connects to `10.0.2.2:8090`
(the host's simulator) instead of BLE. The setup code appears on the mock panel
(`http://127.0.0.1:8080` → 当前画面), and every mock network accepts `inkframe123`. After joining,
the app talks to `10.0.2.2:8080` (`provision.mock_ip`).

## Verified on hardware

2026-10-17, Redmi Note 12 Turbo (Android 15, HyperOS) against the live frame: BLE scan/GATT/
handshake/join through BlueZ, mDNS discovery, every tab, photo upload and preview. Not yet tried:
Android 10/11 (location-permission scan path) and other OEM BLE stacks.

The app says `hello` as soon as it connects, because the frame only paints its setup code after a
hello; waiting for the typed code would leave the panel blank.
