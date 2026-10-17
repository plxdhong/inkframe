#!/usr/bin/env bash
# Build for the Pi Zero 2W and push. Run from the repo root on the dev machine.
#   PI=pi@10.12.0.1 ./deploy/deploy.sh          # over the USB cable
#   PI=frame ./deploy/deploy.sh                 # any ssh host / alias
#   ./deploy/deploy.sh --setup                  # first install: also copies and runs setup-pi.sh
#
# Build: `cross` (static musl) when installed, else Docker with rust:1-bookworm
# (glibc aarch64; native and fast on an Apple Silicon host).
#
# PROXY=http://127.0.0.1:<port> sends crate downloads (Docker build) and the
# device's package installs (--setup) through a proxy on this machine. The
# device reaches it only through an `ssh -R` tunnel for the duration of the
# deploy; nothing proxy-related is written to the device, which runs without one.
set -euo pipefail

PI=${PI:-pi@10.12.0.1}
PROXY=${PROXY:-}
STAGE=target/deploy
TUNNEL_PORT=19877

if command -v cross >/dev/null && [ "${BUILD:-}" != docker ]; then
  TARGET=aarch64-unknown-linux-musl
  cross build --release --target "$TARGET" -p inkframe --features hw,ble
  BIN="target/$TARGET/release/inkframe"
else
  command -v docker >/dev/null || { echo "need cross (cargo install cross --git https://github.com/cross-rs/cross) or docker"; exit 1; }
  TARGET=aarch64-unknown-linux-gnu
  proxy_env=()
  if [ -n "$PROXY" ]; then
    p=${PROXY/127.0.0.1/host.docker.internal}
    p=${p/localhost/host.docker.internal}
    proxy_env=(-e "http_proxy=$p" -e "https_proxy=$p")
  fi
  docker run --rm --platform linux/arm64 -v "$PWD":/src -v inkframe-cargo-registry:/usr/local/cargo/registry \
    -w /src -e CARGO_TARGET_DIR=/src/target/docker "${proxy_env[@]}" rust:1-bookworm \
    cargo build --release --target "$TARGET" -p inkframe --features hw,ble
  BIN="target/docker/$TARGET/release/inkframe"
fi

rm -rf "$STAGE" && mkdir -p "$STAGE/fonts"
cp "$BIN" "$STAGE/"
cp assets/fonts/*.otf assets/fonts/LICENSE-OFL.txt "$STAGE/fonts/"
cp deploy/inkframe.service deploy/setup-pi.sh deploy/inkframe-rw "$STAGE/"
sed -e 's|^data_dir = .*|data_dir = "/data/inkframe"|' \
    -e 's|^fonts_dir = .*|fonts_dir = "/data/inkframe/app/fonts"|' config/inkframe.example.toml > "$STAGE/config.toml"

ssh "$PI" 'mkdir -p /tmp/inkframe-deploy'
rsync -az --delete "$STAGE/" "$PI:/tmp/inkframe-deploy/"
if [ "${1:-}" = "--setup" ]; then
  if [ -n "$PROXY" ]; then
    hostport=${PROXY#*://}
    ssh -t -o ExitOnForwardFailure=yes -R "$TUNNEL_PORT:${hostport%/}" "$PI" \
      "cd /tmp/inkframe-deploy && sudo http_proxy=http://127.0.0.1:$TUNNEL_PORT https_proxy=http://127.0.0.1:$TUNNEL_PORT INKFRAME_DATA_ON_ROOT=${INKFRAME_DATA_ON_ROOT:-0} ./setup-pi.sh"
  else
    ssh -t "$PI" "cd /tmp/inkframe-deploy && sudo INKFRAME_DATA_ON_ROOT=${INKFRAME_DATA_ON_ROOT:-0} ./setup-pi.sh"
  fi
fi
# The app lives on /data, so this works with the read-only root enabled.
ssh "$PI" 'sudo install -d /data/inkframe/app/fonts &&
  sudo install -m 0755 /tmp/inkframe-deploy/inkframe /data/inkframe/app/inkframe.new &&
  sudo mv /data/inkframe/app/inkframe.new /data/inkframe/app/inkframe &&
  sudo cp /tmp/inkframe-deploy/fonts/* /data/inkframe/app/fonts/ &&
  sudo systemctl restart inkframe && sleep 3 && systemctl --no-pager status inkframe | head -5'
