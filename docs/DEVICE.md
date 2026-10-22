# Device bring-up

## 1. Flash

Raspberry Pi Imager 2.x → Raspberry Pi OS Trixie 64-bit Lite. In the customisation step enable SSH,
set Wi-Fi, and enable **USB gadget mode**. Use the data USB port next to HDMI.

## 2. Data partition

Imager expands the root partition on first boot. Before first boot (on another Linux machine), or
by shrinking root afterwards, create an ext4 partition labelled `INKDATA` (≥2 GB):

```bash
sudo parted /dev/sdX -- mkpart primary ext4 -4GiB 100%
sudo mkfs.ext4 -L INKDATA /dev/sdX3
```

`setup-pi.sh` mounts it at `/data` via fstab. Without one, `INKFRAME_DATA_ON_ROOT=1` keeps `/data`
on the root filesystem (no read-only overlay then); the reference unit runs this way.

## 3. Install

```bash
./deploy/deploy.sh --setup        # PI=pi@10.12.0.1 over USB, or pi@<name>.local
PI=pi@10.12.0.1 PROXY=http://127.0.0.1:<port> INKFRAME_DATA_ON_ROOT=1 ./deploy/deploy.sh --setup
```

Without `cross` the build runs in Docker (`rust:1-bookworm`, glibc aarch64). `PROXY` is a proxy on
the dev machine: the Docker build uses it for crates, and `--setup` reaches it from the Pi over an
`ssh -R` tunnel for apt only. Nothing proxy-related is written to the device; the daemon runs
without one (QWeather, holiday mirrors and the probes are all reachable from mainland China).

The setup script enables SPI, turns off Wi-Fi power saving (NetworkManager default **and** every
saved profile), enables the systemd hardware watchdog, makes journald persistent and size-capped
(never age-pruned: the Pi has no RTC, and `MaxRetentionSec` wiped all history when NTP jumped the
clock), disables the old BunCEInk units, gives `usb0` a fixed `10.12.0.1` with DHCP for the laptop,
and installs `inkframe.service`. Then set the `[weather]` credentials in
`/data/inkframe/config.toml` (`api_key`, or `jwt_project_id` + `jwt_key_id` +
`jwt_private_key_path` for Ed25519 JWT) and reboot. Missing credentials are not fatal; the
dashboard just has no weather.

Check first: HAT revision (no PWR pin on the old revision → `display.pins.has_pwr = false`)
and `uname -m` → `aarch64`.

## 4. Bring-up checks

1. `http://10.12.0.1:8080` → 测试：六色色块. All six bars correct and in order?
2. 测试：抖动样张, compare against the panel, adjust `[display.palette]`, then
   `POST /api/v1/photos/reprocess`.
3. Pull the Wi-Fi router: network task escalates reconnect → restart NetworkManager → reload the
   brcmfmac driver → reboot (skipped while the USB cable is connected), repeating for as long as
   the outage lasts. Each reboot that did not help doubles the next reboot threshold (15 min → 4 h).
4. With `http.debug_endpoints = true`: stall a task; systemd should restart the service within ~60 s.

## Why the old build dropped off Wi-Fi (2026-10 diagnosis)

The old BLE flow (2026-07-25) created a new priority-100 profile with `802-11-wireless.powersave =
default`. There was no global NetworkManager override, so brcmfmac power saving came back on. The
last successful render was 2026-07-27; after that the device stayed offline until it was power
cycled, because the old service had no network recovery. The router also deauthenticated it
(reason 6) while power save was on. Earlier logs were gone (see journald above). The daemon now
re-checks every Wi-Fi profile at start and turns power saving off.

## Known issue: slow 2.4 GHz throughput (2026-10-17, shelved)

The link reports 65–130 Mbit/s at -50 dBm on channel 7, but TCP gets only ~30–290 KB/s each way
(3 MB over ssh: 116 s from the Pi, 61 s to it). Idle ping from the Pi to the router is 4–1074 ms
(the Mac on 5 GHz: 6–10 ms). During a transfer TCP resends only ~1% of packets, yet the smoothed
round trip reaches ~1.6 s. So the time is lost in the Wi-Fi radio layer (resends or waiting for
airtime), not in the Pi's software.

Ruled out: Bluetooth (`bluetoothctl power off` changed nothing), power save (off in the driver and
in every profile), channel congestion (only the router's own BSSIDs on ch7), periodic scans, roaming,
SDIO clock (50 MHz), CPU and throttling, HDMI. `retry` in `/proc/net/wireless` is meaningless on
brcmfmac. Still open: the Pi's antenna under the HAT and panel, or the router's 2.4 GHz band. Test a
phone forced onto 2.4 GHz next to the frame; if it is slow too, try the router at 20 MHz on ch
1/6/11 with 802.11ax off on 2.4 GHz. The daemon works fine at this speed (the preview payload was
shrunk for it).

Side note: NetworkManager holds three autoconnect profiles for the same SSID. `BunCEInk-BLE-*` has
priority 100, so it wins after a reboot over `inkframe-<SSID>` (10) and `netplan-wlan0-*` (0).
This is harmless while all three have power save off, but the leftovers could be deleted.


## 5. Lock down

```bash
sudo ./setup-pi.sh --overlay     # read-only root; reboot
sudo inkframe-rw                 # temporarily writable (reboots); `--ro` to lock again
```

App updates via `deploy.sh` keep working with the overlay on, because the binary lives on `/data`.

## 6. Daily use and soak test (from 2026-10-17)

v0.1.0 is in daily use (v0.1.1, remote shutdown/reboot, since 2026-10-22); the 30-day soak (P4) and the open P0 checks run alongside it. Verified on
hardware before this started: dashboard and photo schedule, weather and holidays, photo
upload/preview/rotation from the phone, BLE provisioning, mDNS discovery, watchdog and recovery.

Not done yet, so expect it during use:

- Colours come from the design palette, not the panel. Calibrate with check 2 above when convenient.
- The root filesystem is still writable (`/data` lives on it). Writes are atomic, so a power cut
  should not corrupt state, but SD wear is not limited until step 5.
- The router-off test (check 3) has not been run.

Once or twice a day, from the dev machine:

```bash
PI=pi@10.12.0.1 ./deploy/soak-check.sh        # appends a row to data/soak.csv
```

Watch `rss_kb` and `fds` for steady growth, and `svc_restarts`, `task_restarts` and
`refresh_failures` for any change; each change has a cause in `journalctl -u inkframe`. Any of
these means the soak clock restarts after the fix: the panel showing the wrong thing at a
scheduled time, the same image for more than 24 h, or the frame staying offline after the router
is back.

Rollback: `/data/inkframe/app/inkframe-0.1.0` is the archived binary (copy it over `inkframe` and
restart the service); the old BunCEInk service is still in `/opt/bunceink`.
