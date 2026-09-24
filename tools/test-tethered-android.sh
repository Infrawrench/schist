#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/opt/homebrew/share/android-commandlinetools}}"
if [[ ! -d "$sdk" ]]; then sdk="$HOME/Library/Android/sdk"; fi
android_jar="$(find "$sdk/platforms" -name android.jar | sort -V | tail -1)"
[[ -f "$android_jar" ]] || { echo "Set ANDROID_HOME to an Android SDK" >&2; exit 1; }
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
javac -cp "$android_jar" -d "$stage" \
  packaging/android/java/com/infrawrench/schist/TetheredCamera.java \
  tools/tests/TetheredCameraTest.java
java -cp "$stage:$android_jar" com.infrawrench.schist.TetheredCameraTest
