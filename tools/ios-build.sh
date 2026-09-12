#!/usr/bin/env bash
# Build Schist for the iOS Simulator (or a device) into dist/ios/Schist.app,
# and, for the Simulator, install and launch it.
#
#   tools/ios-build.sh                    # Simulator, release
#   tools/ios-build.sh --debug            # Simulator, debug (much faster)
#   tools/ios-build.sh --device           # arm64 device bundle, unsigned
#   tools/ios-build.sh --no-run           # build and bundle only
#   SIMULATOR_DEVICE="iPad Pro 13-inch (M5)" tools/ios-build.sh
#   SIMULATOR_UDID=<udid> tools/ios-build.sh
#
# The bundle is unsigned: the Simulator accepts that, a device does not.
# Signing, entitlements and an App Store submission go through Xcode's
# tooling on top of this bundle (codesign --sign ... --entitlements ...).
# See docs/ios.md.
set -euo pipefail
cd "$(dirname "$0")/.."

profile=release
profile_flag=--release
run=1
target=aarch64-apple-ios-sim
case "$(uname -m)" in x86_64) target=x86_64-apple-ios ;; esac
for arg in "$@"; do
  case "$arg" in
    --debug) profile=debug; profile_flag= ;;
    --device) target=aarch64-apple-ios; run=0 ;;
    --no-run) run=0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

rustup target list --installed 2>/dev/null | grep -qx "$target" \
  || rustup target add "$target"

echo "-- building schist for $target ($profile)"
cargo build --target "$target" $profile_flag -p schist-app

app=dist/ios/Schist.app
rm -rf "$app"
mkdir -p "$app"
cp "target/$target/$profile/schist" "$app/schist"
cp packaging/ios/Info.plist "$app/Info.plist"
# Keep the bundle's version in step with the workspace's.
version=$(sed -n '0,/^version = /s/^version = "\(.*\)"/\1/p' Cargo.toml)
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $version" \
  -c "Set :CFBundleShortVersionString $version" "$app/Info.plist"
echo "-- bundled $app"

[ "$run" = 1 ] || exit 0

udid="${SIMULATOR_UDID:-}"
if [ -z "$udid" ]; then
  udid=$(xcrun simctl list devices booted -j | python3 -c '
import json, sys
devices = json.load(sys.stdin)["devices"]
booted = [d for runtime in devices.values() for d in runtime if d["state"] == "Booted"]
print(booted[0]["udid"] if booted else "")')
fi
if [ -z "$udid" ]; then
  name="${SIMULATOR_DEVICE:-iPad Pro 13-inch (M5)}"
  udid=$(xcrun simctl list devices available -j | python3 -c '
import json, sys
name = sys.argv[1]
devices = json.load(sys.stdin)["devices"]
matches = [d for runtime in devices.values() for d in runtime if d["name"] == name]
if not matches:
    sys.exit("no simulator named %r; see xcrun simctl list devices" % name)
print(matches[-1]["udid"])' "$name")
  echo "-- booting $name"
  xcrun simctl boot "$udid"
  xcrun simctl bootstatus "$udid" -b >/dev/null
fi
open -a Simulator >/dev/null 2>&1 || true
echo "-- installing"
xcrun simctl install "$udid" "$app"
echo "-- launching (logs follow; ctrl-c leaves the app running)"
xcrun simctl launch --console-pty "$udid" com.infrawrench.schist
