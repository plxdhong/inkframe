#!/usr/bin/env bash
# Runs Gradle for the Android app inside a throwaway container, so building and
# unit-testing needs only Docker (no local JDK / Android SDK).
#
#   app/android/tools/docker-gradle.sh assembleDebug testDebugUnitTest
#
# The SDK, Gradle caches and the debug keystore live in named volumes and survive
# between runs (a fresh keystore per run would make every APK a signature mismatch).
# Android's aapt2 is x86_64-only on Linux, hence --platform linux/amd64.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/../../.." && pwd)"
IMAGE="eclipse-temurin:21-jdk"
CMDLINE_TOOLS="commandlinetools-linux-16111833_latest.zip"
PACKAGES=("platform-tools" "platforms;android-37.0" "build-tools;37.0.0")

docker volume create inkframe-android-sdk >/dev/null
docker volume create inkframe-gradle-home >/dev/null
docker volume create inkframe-android-home >/dev/null

docker run --rm --platform linux/amd64 \
  -v inkframe-android-sdk:/opt/android-sdk \
  -v inkframe-gradle-home:/root/.gradle \
  -v inkframe-android-home:/root/.android \
  -v "$REPO":/repo -w /repo/app/android \
  --add-host host.docker.internal:host-gateway \
  -e INKFRAME_SIM -e INKFRAME_API \
  -e ANDROID_HOME=/opt/android-sdk \
  -e CMDLINE_TOOLS="$CMDLINE_TOOLS" \
  -e PACKAGES="${PACKAGES[*]}" \
  "$IMAGE" bash -euo pipefail -c '
    if [ ! -x "$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" ]; then
      apt-get update -qq && apt-get install -y -qq unzip curl >/dev/null
      curl -sSfL -o /tmp/clt.zip "https://dl.google.com/android/repository/$CMDLINE_TOOLS"
      mkdir -p "$ANDROID_HOME/cmdline-tools" && unzip -q /tmp/clt.zip -d /tmp/clt
      mv /tmp/clt/cmdline-tools "$ANDROID_HOME/cmdline-tools/latest"
    fi
    SDKM="$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager"
    yes | "$SDKM" --sdk_root="$ANDROID_HOME" --licenses >/dev/null 2>&1 || true
    read -ra PKGS <<< "$PACKAGES"
    for p in "${PKGS[@]}"; do
      d="$ANDROID_HOME/$(echo "$p" | tr ";" "/")"
      [ -d "$d" ] || "$SDKM" --sdk_root="$ANDROID_HOME" --install "$p" >/dev/null
    done
    if [ "$#" -eq 0 ]; then "$SDKM" --sdk_root="$ANDROID_HOME" --list_installed; exit 0; fi
    # Docker VMs are often small (OrbStack defaults to 4 GiB): one JVM, modest heap.
    ./gradlew --no-daemon --no-watch-fs --console=plain \
      "-Dorg.gradle.jvmargs=-Xmx2g -Dfile.encoding=UTF-8" \
      -Pkotlin.compiler.execution.strategy=in-process "$@"
  ' _ "$@"
