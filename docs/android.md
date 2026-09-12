# Schist on Android

Schist builds for `aarch64-linux-android` (every device, and the emulator
on an Apple Silicon host) and `x86_64-linux-android` (the emulator on an
Intel host) through gpui's Android backend (see the fork's
`docs/android.md` for the backend itself: a `NativeActivity` hosts the
app, blade renders on Vulkan, touch is mapped onto gpui's mouse model as
on iOS). The same editor runs; what changes is the input, the chrome,
where files live, and which subsystems the app carries.

## Building and running

```sh
tools/android-build.sh --debug       # arm64, debug: fast to build
tools/android-build.sh               # arm64, release
tools/android-build.sh --x86_64      # for an emulator on an Intel host
tools/android-build.sh --no-run      # build and package only (make android)
AVD_NAME=pixel tools/android-build.sh --debug
```

The script builds the app crate as a shared library for the target,
packages `dist/android/Schist.apk` from `packaging/android/` (the
manifest, the launcher icon) with the SDK's `aapt2`, `zipalign` and
`apksigner`, and, unless told not to, installs it on the connected device
or a running emulator -- booting one if there is neither, created from
the newest installed system image -- launches it, and follows its log.
The APK is signed with the SDK's debug key, which a device accepts from
`adb` and a store does not; a store build re-signs the same APK with a
release key.

Requirements: the Android SDK with `platform-tools`, `build-tools`, a
platform, an NDK, and, to run without a device, `emulator` and a system
image; the Rust target the script adds. From Homebrew:

```sh
brew install --cask android-commandlinetools temurin
sdkmanager --install "platform-tools" "build-tools;35.0.0" "platforms;android-35" \
    "ndk;27.2.12479018" "emulator" "system-images;android-35;google_apis;arm64-v8a"
```

The script finds the SDK under the usual locations (`ANDROID_HOME` names
it otherwise) and the newest NDK under it (`ANDROID_NDK_HOME` otherwise).
`cargo check --target aarch64-linux-android` by hand needs the NDK's
clang as the target's compiler and linker, because the `android-activity`
glue in gpui compiles one C file: the variables are the ones the script
exports (`CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER`,
`CC_aarch64_linux_android`, `AR_aarch64_linux_android`), and CI's
"Android cross-check" step shows them.

**The library and the binary.** Android runs an app as a shared library
that its `NativeActivity` loads, calling the `android_main` that
`gpui::android_main!` defines; `rustc` cannot build one crate as both an
executable and a shared library. So the app crate is a library
(`crates/app/src/lib.rs`, everything) with a one-line executable in front
of it (`src/main.rs`) on every other platform, and the Android build asks
for the cdylib with `cargo rustc --lib --crate-type cdylib` rather than a
`crate-type` in `Cargo.toml`, which would make every platform link a
library nothing loads.

## What is different

**Input.** As on iOS: one finger on the canvas paints, drags or pans,
whatever the active tool would do with the mouse; two fingers pan and
pinch to zoom; one finger scrolls every list and panel, with momentum;
a long press is a right click and opens the same context menus a
desktop right click does, except on the canvas, where the tool owns the
press. A stylus draws with pressure and its barrel button right-clicks;
a mouse is a mouse. A hardware keyboard drives the whole keymap with
Ctrl where macOS has Command (Android reserves the Meta key for its own
shortcuts). The software keyboard comes up for any field that is taking
typing and goes when it lets go, through the same input-handler bridge
as iOS (`crates/app/src/workspace/text_input.rs`); it types through key
events, so letters, digits, punctuation, Return and Backspace arrive and
autocorrect, suggestions and IME composition do not (a `NativeActivity`
offers the input method no `InputConnection`). Back is Escape: it closes
a dialog or a menu, and with nothing open leaves the app.

**Chrome.** The touch chrome, shared with iOS: the window fills the
screen, the root pads by `Window::safe_area_insets()` (status bar,
navigation bar or gesture area, a display cutout, the keyboard while it
is up), and everything sized for a pointer grows to a 44pt target
(`ui::touch()`). A phone-width window folds the side panels and the
gallery strip as an iPhone does (`ui::compact()`); a tablet has the
desktop's layout at touch sizes.

**Menus.** Android has no menu bar of its own and no native menus, so
the in-window bar is always drawn, and it opens the desktop's own
drop-downs: a tap on a title opens its menu (a tap rather than a press,
so a swipe along the bar, which scrolls it, opens nothing), and a tap on
a "Name ›" row opens its submenu, which on the desktop opens on hover.

**Files.** There is no system file picker either: a `NativeActivity`
cannot receive the activity result one answers with. Every prompt for a
path -- Open, Save As, Export, Add Folder to Gallery, the cloud's
uploads and downloads -- goes through `Workspace::prompt_for_paths` and
`prompt_for_new_path`, which on Android open a dialog of Schist's own
(`crates/app/src/workspace/file_picker.rs`, drawn by
`dialogs/file_picker.rs`): a listing of the folder to walk, folders
first, a row of places (the app's Documents, then the device's Pictures,
camera roll and Downloads), Up, and for a save a name field. Tapping a
file picks it (several, when the prompt allows), tapping a folder enters
it, and a folder chooser takes the folder walked to. The desktop and iOS
keep their platform dialogs behind the same two calls.

**Where things live.** An app's process starts with no `HOME`, so the
app sets one before anything derives a path from it
(`crates/app/src/android.rs`): the app's private files directory
(`/data/user/0/com.infrawrench.schist/files`), under which preferences
and the library sit in `.config/schist` and the caches, recovery files
and index in `.local/state/schist`, as on Linux; `TMPDIR` is the cache
directory beside it. Documents are elsewhere: `Documents` under the
app's external files directory
(`/sdcard/Android/data/com.infrawrench.schist/files`), which is still
the app's own but reachable over USB and `adb`, so what the user saves
can be got at; the picker starts there, and a path under it shows from
`Documents` down. The device's shared folders (Pictures, DCIM, Download)
are listed as places but reading them needs the storage permission,
which the app does not yet ask for (a permission request's answer is
another activity result); the picker says so when a folder cannot be
read.

**HEIC** decodes through the same downloaded libheif as the desktop:
[IAmJSD/libheif-prebuilt](https://github.com/IAmJSD/libheif-prebuilt)
publishes Android builds (arm64 and x86_64, with the NDK's libc++ linked
in so they depend on Bionic alone) from its v1.23.2-4 release, pinned
by hash in `plugins/codecs-common/src/heif.rs` like the others. The
download lands under the private files directory, which is the one
place on the device a library can be `dlopen`ed from (external storage
is mounted `noexec`).

**Cloud sign-in.** The login is kept by gpui's keystore-backed
credentials: an AES key generated in the Android keystore encrypts it
into the app's private storage. The browser's `schist://` callback
reaches the app through its launch intent when the browser starts it;
an app already running is not told (a `NativeActivity` does not see a
new intent), so a sign-in started from a running app has to be finished
by closing and reopening it.

**Compiled out.** Everything under the `sandboxed` cfg that
`crates/app/build.rs` sets for iOS and the browser is off on Android
too, for a different reason: Android could run subprocesses, `dlopen`
and a JIT, but there is nothing for them to run. The Photoshop plug-in
helpers are desktop binaries for other architectures, the agent CLIs
behind the AI sidebar are desktop installs, and the store updates the
app. So: the Photoshop and WebAssembly plug-in hosts (the first-party
plugins are all there), the AI sidebar and the MCP bridge, the
self-updater, dragging photos out to a file manager, and Quit. The
iOS-only pieces stay iOS-only: Save to Photos, Import from Photos (the
camera roll needs the photo picker, another activity result), and the
share-sheet handover.

Like iOS, Android keeps the gallery, the GPU compositor (`wgpu` on its
own Vulkan device; gpui's renderer is separate), the crash reporter, the
daily ping, font and model downloads, and the cloud.

## Status

Compiles and type-checks for `aarch64-linux-android` (CI's "Android
cross-check" step). Checked by hand in the Android 15 emulator (API 35,
Pixel Tablet profile, SwiftShader Vulkan) with `tools/android-build.sh
--debug`: launch, the gallery's welcome screen inside the safe area, the
model download, the menu bar and a submenu opening on taps, the file
picker opening a PNG pushed into Documents, the editor rendering it on
the CPU compositor, and Save As typing a name through the software
keyboard (which comes and goes with the field, the dialog resizing above
it) and writing the PSD. No physical device has run it yet: a real
Vulkan driver, the GPU compositor (which the emulator's software Vulkan
never gets, see below), a real input method, a stylus, the cloud
sign-in's keystore-backed login and the libheif download are unverified.

Two things the emulator taught:

- Its Vulkan device is SwiftShader on the host, which takes minutes to
  compile the compositor's kernels and is slower than the CPU compositor
  once it has. The GPU compositor is opened on its own thread on Android
  (`crates/app/src/lib.rs`), so the activity starts while it is prepared
  and the CPU compositor serves until then, and it declines a software
  Vulkan device altogether (`crates/compositor-gpu/src/exec.rs`, Android
  only); the log says so. gpui's own rendering still goes through
  SwiftShader there, so a frame takes seconds in the emulator and
  `adb shell input text` drops keys the app has not acknowledged in time
  -- type with one `keyevent` per key when driving it from a terminal.
- A dialog is an absolute overlay, so the root's inset padding did not
  reach it and the software keyboard covered half of it. The overlay is
  now inset the same way, and the picker sizes its listing from
  `Workspace::visible_height`, which the render records each frame.

What a device would still change: the storage permission (so the shared
Pictures, camera roll and Downloads can be read), a Save to Photos
through `MediaStore` (an insert needs no activity result), and a launch
intent carrying a `content://` URI, which `path_from_url` does not open.
