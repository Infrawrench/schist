#!/bin/sh
# Runs a crate's test binaries inside a booted iOS Simulator.
#
# cargo builds the tests for the Simulator target; each binary is wrapped
# in a minimal .app bundle, installed, and launched with its output on
# this terminal. That is how the GPU compositor's parity tests are run
# against the Simulator's Metal GPU rather than the Mac's.
#
#   tools/ios-test.sh -p schist-compositor-gpu
#   SIMULATOR_UDID=<udid> tools/ios-test.sh -p schist-compositor-gpu
set -eu
cd "$(dirname "$0")/.."
target=aarch64-apple-ios-sim
udid="${SIMULATOR_UDID:-$(xcrun simctl list devices booted -j | python3 -c 'import json,sys; d=json.load(sys.stdin); print(next(dev["udid"] for devs in d["devices"].values() for dev in devs))' 2>/dev/null || true)}"
if [ -z "$udid" ]; then
    echo "no booted Simulator; boot one or set SIMULATOR_UDID" >&2
    exit 1
fi
out=$(mktemp -d)
status=0
cargo test --no-run --target "$target" "$@" 2>&1 | tee "$out/build.log" >&2
grep -E '^\s*Executable' "$out/build.log" | sed -E 's/.*\((.*)\)$/\1/' | while read -r bin; do
    name=$(basename "$bin" | sed -E 's/-[0-9a-f]+$//')
    app="$out/$name.app"
    mkdir -p "$app"
    cp "$bin" "$app/$name"
    id="com.infrawrench.schist.test.$name"
    cat > "$app/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>$name</string>
<key>CFBundleIdentifier</key><string>$id</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>1</string>
<key>CFBundleExecutable</key><string>$name</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>MinimumOSVersion</key><string>16.0</string>
<key>LSRequiresIPhoneOS</key><true/>
<key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
<key>UILaunchScreen</key><dict/>
</dict></plist>
PLIST
    echo "== $name" >&2
    xcrun simctl install "$udid" "$app"
    # The harness's summary is the verdict; the launch itself exits 0
    # whatever the tests did.
    xcrun simctl launch --console-pty "$udid" "$id" | tee "$out/$name.log"
    xcrun simctl uninstall "$udid" "$id" || true
    if ! grep -q '^test result: .*ok' "$out/$name.log" || grep -q 'FAILED' "$out/$name.log"; then
        echo "$name: FAILED" >&2
        touch "$out/failed"
    fi
done
[ ! -e "$out/failed" ] || status=1
rm -rf "$out"
exit $status
