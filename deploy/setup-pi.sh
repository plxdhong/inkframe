#!/usr/bin/env bash
# One-time device preparation. Run on the Pi as root:  sudo ./setup-pi.sh
# Idempotent. Enable the read-only root last, separately:  sudo ./setup-pi.sh --overlay
# Without an INKDATA partition, INKFRAME_DATA_ON_ROOT=1 keeps /data on the root
# filesystem instead (the read-only overlay is then not possible).
# Only package installs use a proxy, and only if http_proxy is set by the caller
# (deploy.sh tunnels one); nothing proxy-related is persisted on the device.
set -euo pipefail

DATA=/data
APP=$DATA/inkframe
USB_ADDR=10.12.0.1/24

log() { printf '\033[1m==> %s\033[0m\n' "$*"; }
[ "$(id -u)" = 0 ] || { echo "run as root"; exit 1; }

if [ "${1:-}" = "--overlay" ]; then
  log "Enabling read-only root (overlayfs); reboot required"
  raspi-config nonint enable_overlayfs
  echo "Use 'sudo inkframe-rw' to make the root writable again (reboots)."
  exit 0
fi

log "Enable SPI"
raspi-config nonint do_spi 0

log "Disable Wi-Fi power saving (what took the old builds off Wi-Fi)"
install -d /etc/NetworkManager/conf.d
cat > /etc/NetworkManager/conf.d/inkframe-wifi-powersave.conf <<'CONF'
[connection]
wifi.powersave = 2
CONF
# The global default only covers profiles left at "default"; set it on every
# saved profile too (the old BLE flow created one with powersave=default that
# overrode the Imager profile). Takes effect on the next activation.
nmcli -t -f UUID,TYPE connection show | while IFS=: read -r uuid type; do
  [ "$type" = 802-11-wireless ] && nmcli connection modify "$uuid" 802-11-wireless.powersave 2
done
nmcli general reload conf || true

log "Bluetooth for phone provisioning (BlueZ GATT server)"
APT_OPTS=(-o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30)
# An explicit proxy beats any stale one in /etc/apt/apt.conf.d.
[ -n "${http_proxy:-}" ] && APT_OPTS+=(-o "Acquire::http::Proxy=$http_proxy" -o "Acquire::https::Proxy=$http_proxy")
command -v bluetoothctl >/dev/null || apt-get "${APT_OPTS[@]}" install -y bluez
systemctl enable --now bluetooth
# Keep the controller up so advertising can start without user interaction.
install -d /etc/bluetooth
grep -q '^AutoEnable=true' /etc/bluetooth/main.conf 2>/dev/null || {
  sed -i 's/^#\?AutoEnable=.*/AutoEnable=true/' /etc/bluetooth/main.conf 2>/dev/null || true
  grep -q '^AutoEnable=true' /etc/bluetooth/main.conf 2>/dev/null || printf '[Policy]\nAutoEnable=true\n' >> /etc/bluetooth/main.conf
}

log "Hardware watchdog via systemd"
install -d /etc/systemd/system.conf.d
cat > /etc/systemd/system.conf.d/inkframe-watchdog.conf <<'CONF'
[Manager]
RuntimeWatchdogSec=15
RebootWatchdogSec=2min
CONF

log "journald persistent, size-capped, never pruned by age"
# Volatile logs lost every trace of the last outage at the reboot that ended it.
# Age-based pruning is worse: the Pi has no RTC, boots with weeks-old time, and
# MaxRetentionSec then deletes everything (including the current boot) once NTP
# jumps the clock forward. Cap by size only; the 99- prefix overrides others.
install -d /etc/systemd/journald.conf.d /var/log/journal
rm -f /etc/systemd/journald.conf.d/inkframe.conf
cat > /etc/systemd/journald.conf.d/99-inkframe.conf <<'CONF'
[Journal]
Storage=persistent
SystemMaxUse=48M
SystemKeepFree=500M
MaxRetentionSec=0
MaxFileSec=1month
SyncIntervalSec=5min
CONF
systemctl restart systemd-journald

log "Stop the previous BunCEInk services (they drive the same panel)"
for u in bunceink-wifi-apply.path bunceink-power-apply.path; do
  systemctl disable --now "$u" 2>/dev/null || true
done
if [ -n "${SUDO_USER:-}" ] && [ -d "/home/$SUDO_USER/.config/systemd/user" ]; then
  uid=$(id -u "$SUDO_USER")
  for u in bunceink.service bunceink-ble.service bunceink-display-boot.service \
           bunceink-display-morning-weather.timer bunceink-display-evening-weather.timer; do
    runuser -u "$SUDO_USER" -- env XDG_RUNTIME_DIR="/run/user/$uid" systemctl --user disable --now "$u" 2>/dev/null || true
  done
fi

log "Data partition"
if mountpoint -q "$DATA"; then
  echo "$DATA is mounted"
elif blkid -L INKDATA >/dev/null 2>&1; then
  install -d "$DATA"
  grep -q 'LABEL=INKDATA' /etc/fstab || echo 'LABEL=INKDATA /data ext4 defaults,noatime,commit=60 0 2' >> /etc/fstab
  mount "$DATA"
elif [ "${INKFRAME_DATA_ON_ROOT:-0}" = 1 ]; then
  echo "No INKDATA partition: using $DATA on the root filesystem (no read-only overlay)."
  install -d "$DATA"
else
  echo "No partition labelled INKDATA. Create one (see docs/DEVICE.md) and re-run," >&2
  echo "or re-run with INKFRAME_DATA_ON_ROOT=1 to keep /data on the root filesystem." >&2
  exit 1
fi

log "USB gadget link: fixed $USB_ADDR with DHCP for the host"
if nmcli -t -f NAME connection show | grep -qx inkframe-usb; then
  echo "connection exists"
else
  nmcli connection add type ethernet ifname usb0 con-name inkframe-usb \
    ipv4.method shared ipv4.addresses "$USB_ADDR" ipv6.method ignore connection.autoconnect yes
fi

log "Install service"
install -d "$APP/app"
[ -f "$APP/config.toml" ] || install -m 0644 config.toml "$APP/config.toml"
install -m 0644 inkframe.service /etc/systemd/system/inkframe.service
install -m 0755 inkframe-rw /usr/local/sbin/inkframe-rw
systemctl daemon-reload
systemctl enable inkframe.service

log "Done. Reboot once so watchdog/journald settings apply."
