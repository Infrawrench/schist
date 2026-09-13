#!/usr/bin/env bash
# Build Schist for Android into dist/android/Schist.apk and, unless told
# not to, install and launch it on the connected device or a running
# emulator, booting one if there is neither, with its log on the terminal.
#
#   tools/android-build.sh                # arm64, release
#   tools/android-build.sh --debug        # arm64, debug (much faster)
#   tools/android-build.sh --x86_64       # for an emulator on an Intel host
#   tools/android-build.sh --no-run       # build and package only
#   AVD_NAME=pixel tools/android-build.sh # the emulator to boot or create
#
# Environment: ANDROID_HOME (the SDK: platform-tools, build-tools, a
# platform, and emulator plus a system image to boot one; found under the
# usual locations when unset), ANDROID_NDK_HOME (the newest under
# $ANDROID_HOME/ndk when unset), EMULATOR_FLAGS (extra flags for a boot).
#
# The APK is signed with the SDK's debug key, which a device accepts
# from adb and a store does not; a store build re-signs this APK with a
# release key (apksigner sign --ks ...). See docs/android.md.
set -euo pipefail
cd "$(dirname "$0")/.."

profile=release
profile_flag=--release
run=1
check=0
video_test=0
abi=arm64-v8a
for arg in "$@"; do
  case "$arg" in
    --debug) profile=debug; profile_flag= ;;
    --x86_64) abi=x86_64 ;;
    --no-run) run=0 ;;
    --check) check=1; run=0 ;;
    --video-test) video_test=1; run=0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done
case "$abi" in
  arm64-v8a) target=aarch64-linux-android ;;
  x86_64) target=x86_64-linux-android ;;
esac

# The SDK, and the tools on it.
if [ -z "${ANDROID_HOME:-}" ]; then
  ANDROID_HOME="${ANDROID_SDK_ROOT:-}"
fi
if [ -z "$ANDROID_HOME" ]; then
  for candidate in "$HOME/Library/Android/sdk" "$HOME/Android/Sdk" \
      /opt/homebrew/share/android-commandlinetools \
      /usr/local/share/android-commandlinetools; do
    if [ -d "$candidate" ]; then
      ANDROID_HOME="$candidate"
      break
    fi
  done
fi
if [ ! -d "$ANDROID_HOME" ]; then
  echo "set ANDROID_HOME to the Android SDK" >&2
  exit 1
fi
export ANDROID_HOME
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  ANDROID_NDK_HOME=$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)
fi
if [ ! -d "${ANDROID_NDK_HOME:-}" ]; then
  echo "no NDK: set ANDROID_NDK_HOME, or sdkmanager --install 'ndk;27.2.12479018'" >&2
  exit 1
fi
export ANDROID_NDK_HOME
build_tools=$(ls -d "$ANDROID_HOME"/build-tools/* 2>/dev/null | sort -V | tail -1)
platform_jar=$(ls "$ANDROID_HOME"/platforms/android-*/android.jar 2>/dev/null | sort -V | tail -1)
if [ -z "$build_tools" ] || [ -z "$platform_jar" ]; then
  echo "sdkmanager --install 'build-tools;35.0.0' 'platforms;android-35'" >&2
  exit 1
fi
PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$ANDROID_HOME/cmdline-tools/latest/bin:$build_tools:$PATH"
export PATH

# The NDK's clang as the target's C compiler and linker: the
# android-activity glue in gpui compiles one C file, and the linker has
# to be the one that knows Bionic. 30 is the manifest's minSdkVersion.
host=$(ls "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt")
toolchain="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$host/bin"
target_upper=$(echo "$target" | tr 'a-z-' 'A-Z_')
target_lower=$(echo "$target" | tr '-' '_')
export "CARGO_TARGET_${target_upper}_LINKER=$toolchain/${target}30-clang"
export "CC_${target_lower}=$toolchain/${target}30-clang"
export "CXX_${target_lower}=$toolchain/${target}30-clang++"
export "AR_${target_lower}=$toolchain/llvm-ar"

rustup target list --installed 2>/dev/null | grep -qx "$target" \
  || rustup target add "$target"

compile_android_java() {
  local stage="$1"
  # The native activity bridge, video decoder, URI provider and backup service. javac comes with the JDK the SDK tools need
  # anyway; d8 is in build-tools.
  if ! command -v javac >/dev/null; then
    echo "javac not found: install a JDK (brew install --cask temurin)" >&2
    exit 1
  fi
  mkdir -p "$stage/classes"
  javac -source 8 -target 8 -Xlint:-options -bootclasspath "$platform_jar" \
    -classpath "$build_tools/core-lambda-stubs.jar" -d "$stage/classes" \
    packaging/android/java/com/infrawrench/schist/*.java
  d8 --release --min-api 30 --lib "$platform_jar" --output "$stage" \
    "$stage"/classes/com/infrawrench/schist/*.class
}

# Test the packaged decoder on a connected emulator/device, without starting
# the UI. This runs Android's real MediaCodec implementation, not desktop mocks.
if [ "$video_test" = 1 ]; then
  stage="target/android-video-test"
  compile_android_java "$stage"
  javac -source 8 -target 8 -Xlint:-options -bootclasspath "$platform_jar" \
    -classpath "$stage/classes:$build_tools/core-lambda-stubs.jar" -d "$stage/classes" \
    tools/tests/VideoDecoderTest.java
  d8 --release --min-api 30 --lib "$platform_jar" --output "$stage" \
    "$stage"/classes/com/infrawrench/schist/*.class
  remote="/data/local/tmp/schist-video-test-$$"
  adb shell mkdir -p "$remote"
  trap 'adb shell rm -rf "$remote" >/dev/null 2>&1 || true' EXIT
  adb push "$stage/classes.dex" crates/app/tests/fixtures/video/*.mp4 "$remote/" >/dev/null
  adb shell "CLASSPATH=$remote/classes.dex app_process / com.infrawrench.schist.VideoDecoderTest $remote"
  exit 0
fi

# NativeActivity loads a shared library, so the app crate is built as
# one: the cdylib crate type is passed here rather than set in Cargo.toml,
# where it would make every other platform link a library it never uses.
echo "-- building schist for $target ($profile)"
if [ "$check" = 1 ]; then
  cargo check -p schist-app --target "$target"
  compile_android_java "target/$target/camera-sync-java-check"
  exit 0
fi
cargo rustc -p schist-app --lib --crate-type cdylib --target "$target" $profile_flag

lib="target/$target/$profile/libschist_app.so"
stage="target/$target/$profile/apk"
apk=dist/android/Schist.apk
rm -rf "$stage"
mkdir -p "$stage/lib/$abi" dist/android
cp "$lib" "$stage/lib/$abi/"
# Keep the APK's version in step with the workspace's. versionCode has
# to be an integer that only ever grows: major, minor and patch, two
# digits each.
version=$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)
: "${version:?missing workspace version}"
code=$(echo "$version" | awk -F. '{ printf "%d", $1 * 10000 + $2 * 100 + $3 }')
echo "-- packaging $apk ($version, versionCode $code)"
aapt2 compile --dir packaging/android/res -o "$stage/res.zip"
aapt2 link -o "$stage/unaligned.apk" \
  --manifest packaging/android/AndroidManifest.xml \
  --version-name "$version" --version-code "$code" \
  -I "$platform_jar" "$stage/res.zip"
compile_android_java "$stage"
(cd "$stage" && zip -q -r unaligned.apk lib classes.dex)
zipalign -f -p 4 "$stage/unaligned.apk" "$stage/aligned.apk"
keystore="$HOME/.android/debug.keystore"
if [ ! -f "$keystore" ]; then
  mkdir -p "$HOME/.android"
  keytool -genkeypair -keystore "$keystore" -alias androiddebugkey \
    -storepass android -keypass android -keyalg RSA -keysize 2048 \
    -validity 10000 -dname "CN=Android Debug,O=Android,C=US" >/dev/null 2>&1
fi
apksigner sign --ks "$keystore" --ks-pass pass:android --ks-key-alias androiddebugkey \
  --key-pass pass:android --out "$apk" "$stage/aligned.apk"
echo "-- built $apk"

[ "$run" = 1 ] || exit 0

# A device or a running emulator; otherwise boot one, creating it from
# the newest installed system image for this ABI if it does not exist.
if ! adb devices | grep -q "device$"; then
  avd="${AVD_NAME:-schist}"
  if ! emulator -list-avds | grep -qx "$avd"; then
    image=$(ls -d "$ANDROID_HOME"/system-images/android-*/google_apis/"$abi" 2>/dev/null | sort -V | tail -1)
    if [ -z "$image" ]; then
      echo "no system image: sdkmanager --install 'system-images;android-35;google_apis;$abi'" >&2
      exit 1
    fi
    image_id=$(echo "$image" | sed -e "s|.*/system-images/|system-images;|" -e "s|/|;|g")
    echo "-- creating the $avd emulator from $image_id"
    echo no | avdmanager create avd -n "$avd" -k "$image_id" -d pixel_tablet >/dev/null
  fi
  echo "-- booting the $avd emulator"
  # A software Vulkan device on the host: the emulator's default GPU
  # mode advertises no Vulkan at all.
  flags="-no-boot-anim -no-snapshot -feature Vulkan -gpu swiftshader_indirect ${EMULATOR_FLAGS:-}"
  # shellcheck disable=SC2086
  nohup emulator -avd "$avd" $flags >"target/emulator-$avd.log" 2>&1 &
  adb wait-for-device
  until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do
    sleep 2
  done
fi

echo "-- installing"
adb install -r "$apk" >/dev/null
adb logcat -c
echo "-- launching (logs follow; ctrl-c leaves the app running)"
adb shell am start -W -n com.infrawrench.schist/.SchistActivity >/dev/null
# Schist's own output: stdout, stderr and panics reach logcat through
# gpui, plus the activity's and the runtime's messages.
adb logcat -v time gpui-stdout:V gpui-stderr:V NativeActivity:V AndroidRuntime:E DEBUG:V '*:S'
