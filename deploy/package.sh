#!/usr/bin/env bash
# Archives a release: the device bundle that deploy.sh staged, the Android APK,
# the docs needed to reinstall, and the live device's config. Build first:
#   PI=frame ./deploy/deploy.sh
#   app/android/tools/docker-gradle.sh testDebugUnitTest assembleDebug
#   PI=frame ./deploy/package.sh           # → dist/inkframe-<version>.tar.gz
# The device's config is copied without anything under secret/.
set -euo pipefail

PI=${PI:-}
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
STAGE=target/deploy
APK=app/android/app/build/outputs/apk/debug/app-debug.apk
NAME="inkframe-$VERSION"
OUT="dist/$NAME"

[ -x "$STAGE/inkframe" ] || { echo "no $STAGE/inkframe: run deploy/deploy.sh first"; exit 1; }
[ -f "$APK" ] || { echo "no $APK: run docker-gradle.sh assembleDebug first"; exit 1; }

rm -rf "$OUT" && mkdir -p "$OUT/device" "$OUT/android" "$OUT/docs"
cp -R "$STAGE/." "$OUT/device/"
cp "$APK" "$OUT/android/$NAME-debug.apk"
cp docs/DEVICE.md docs/PROVISIONING.md "$OUT/docs/"
cp config/inkframe.example.toml "$OUT/docs/"
if [ -n "$PI" ]; then
  ssh -o ConnectTimeout=10 "$PI" 'cat /data/inkframe/config.toml' > "$OUT/device/config.device.toml"
fi

commit=$(git rev-parse --short HEAD)
dirty=$(git status --porcelain --untracked-files=no | grep -q . && echo " (uncommitted changes)" || true)
cat > "$OUT/RELEASE.txt" <<EOF
inkframe $VERSION
commit $commit$dirty, packaged $(date '+%F %T')

device/      daemon (aarch64, features hw,ble), fonts, systemd unit, setup scripts,
             config.toml (example defaults with /data paths), config.device.toml (live config)
android/     phone app, debug-signed (install over earlier builds from the same Docker volume)
docs/        DEVICE.md (install, checks, daily use), PROVISIONING.md, example config

Install on a fresh Pi: copy device/ to /tmp/inkframe-deploy and run setup-pi.sh (see DEVICE.md §3),
or from the repo at this commit run deploy/deploy.sh --setup.
EOF

(cd "$OUT" && find . -type f ! -name SHA256SUMS | sort | xargs shasum -a 256 > SHA256SUMS)
tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"
shasum -a 256 "dist/$NAME.tar.gz" | tee "dist/$NAME.tar.gz.sha256"
du -h "dist/$NAME.tar.gz"
