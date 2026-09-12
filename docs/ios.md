# Schist on iOS and iPadOS

Schist builds for `aarch64-apple-ios` and the Simulator targets through
gpui's iOS backend (see the fork's `docs/ios.md` for the backend itself:
UIKit hosts the app, Metal renders, touch is mapped onto gpui's mouse
model). The same editor runs; what changes is the input, the chrome, and
which subsystems a sandboxed app can carry.

## Building and running

```sh
tools/ios-build.sh --debug        # Simulator, debug: fast to build
tools/ios-build.sh                # Simulator, release
tools/ios-build.sh --device       # an unsigned arm64 bundle in dist/ios/
SIMULATOR_DEVICE="iPhone 17 Pro" tools/ios-build.sh --debug
```

The script builds `schist-app` for the target, assembles
`dist/ios/Schist.app` from `packaging/ios/Info.plist`, and for the
Simulator boots one (an iPad Pro by default), installs the bundle and
launches it with its logs on the terminal. Requirements: a macOS host
with Xcode and its iOS platform, and the rustup targets (the script adds
them). A device build needs signing on top: an Apple development
certificate, an entitlements file (`keychain-access-groups` for the
cloud sign-in's keychain items), and `codesign` over the bundle, or an
Xcode project that links the binary as a static library.

## What is different

**Input.** One finger on the canvas paints, drags or pans, whatever the
active tool would do with the mouse; the canvas claims single-finger
drags (`Window::claim_touch_drag`) so they are never scrolls. Two fingers
pan the canvas and pinch to zoom, and one finger scrolls every list and
panel. A long press is a right click, so it opens the same context menus
a desktop right click does, except on the canvas, where the active tool
owns the press (a long press with the brush is a dab, as on paper); an
iPad trackpad or mouse is a mouse, and right-clicks the canvas. A
hardware keyboard drives the whole keymap, and the app menus Schist sets
for macOS come up as the iPadOS menu bar and its Command-key shortcut
overlay. The software keyboard comes up for any field that is taking
typing (a dialog's field, the gallery's search box, a layer being
renamed, a note, the type tool) and goes when it lets go: Schist's
fields keep their own buffers and take keys, so
`crates/app/src/workspace/text_input.rs` registers a gpui input handler
while one is active (that is what raises the keyboard) and replays what
the keyboard types as the key events a hardware keyboard would have
sent. Autocorrect's rewrite of the word at the caret is backspaced and
retyped; the inline preview of a composing keyboard (Japanese, Chinese)
is not drawn, the text arrives when it commits.

**Chrome.** The window fills the screen; the workspace pads its root by
`Window::safe_area_insets()`, so the menu bar sits below the status bar
and nothing hides under the home indicator or the keyboard. Everything
sized for a pointer grows to Apple's 44pt target on iOS: the tool slots,
menu rows, layer and history rows, tabs, sliders and the panel column's
buttons, with larger type throughout. The two size tables sit side by
side in `crates/app/src/ui.rs` (`DESKTOP_METRICS` and `TOUCH_METRICS`),
and `ui::touch()` is the one switch. Documents open through the system
document picker and save through it too; the Documents folder is visible
in the Files app (`UIFileSharingEnabled`) and the gallery watches it.

**Menus.** iPadOS has a menu bar of its own, a swipe down from the top
edge or the pointer, fed by the same menus Schist sets for macOS, so an
iPad draws no menu bar in the window (`ui::ipad()`, from the device's
interface idiom). The phone has no such bar and keeps the in-window
one. Its drop-downs open their submenus on hover, which a finger cannot
do, so on iOS each title opens the same menu as the platform's own: a
popover anchored to the title on iPad, a sheet on iPhone. A submenu is a "Name ›" row that opens the next sheet, which
starts with a row back to its parent. The rows are
gpui `MenuItem`s built by the same code as the macOS menu bar, dispatched
through `Window::show_context_menu`. The bar scrolls sideways under a
finger, so on a phone the titles past the edge are a swipe away, and a
title opens on a tap rather than a press so a swipe that starts on one
never opens it. The iPadOS menu bar and Command-key overlay come from the
same menus, set natively.

**Gallery strip.** On a phone-width window the row of buttons above
the grid folds to one row: the drawer button, Import (the thing the
gallery is for), the search box once its models are installed (or their
download's progress), and a "⋯" button at the right end whose menu holds
the rest: Add Folder, Refresh, Enable Photo Search, Settings, Open, New
File and Back to Editing. An iPad has the width for the desktop's full
row and shows it. The two share their actions (`strip_actions` in
`gallery_chrome.rs`). The gallery's menus
are anchored in window coordinates and pushed back inside the window
when they would leave it.

**History panel.** Its height is dragged from the grip along its top
edge rather than fixed: taller for a long session, shorter to give the
layers room, from 120pt to 500pt, kept with the view preferences. The
list scrolls inside whatever height it has instead of running past the
panel's edge. The layers panel gives way down to 120pt; past that the
column scrolls, as it does under a tall Info tab.

**Thumbnail size.** The tray's size slider is gone on iOS: a pinch on
the grid grows or shrinks the thumbnails, the way ⌘-wheel does on the
desktop, and the two-finger scroll still scrolls.

**Symbols in text.** The chrome writes a few symbols as text: the
cloud on cloud rows, the star on smart buckets, the ✕ on chips. The
iOS system font has none of them, and CoreText's own cascade reaches
for the emoji font first, which draws them as emoji on a device and as
the missing-glyph box in the Simulator. The workspace root names Apple
Symbols and Zapf Dingbats (both on every iOS) as the text's fallback
fonts, so they render as the desktop's monochrome glyphs.

**Symbols in text.** The chrome writes a few symbols as text: the
cloud on cloud rows, the star on smart buckets, the ✕ on chips. The
iOS system font has none of them, and CoreText's own cascade reaches
for the emoji font first, which draws them as emoji on a device and as
the missing-glyph box in the Simulator. The workspace root names Apple
Symbols and Zapf Dingbats (both on every iOS) as the text's fallback
fonts, so they render as the desktop's monochrome glyphs.

**Paths.** The app's container is a long opaque string that changes
on every install, so iOS never shows a path whole: the status bar's
"Opened", "Saved", "Exported" and "Imported" messages, the folder
grouping's titles and the cloud's download notes show the part under
the container ("Documents/Photos/IMG_0111.heic"), anything elsewhere by
its name (`ui::shown_path`). Preferences drops its keymap-file line,
which named a file nothing on iOS can open.

**Dialogs.** Every dialog asks for a width made for a desktop window;
on a phone it gets the window's width less a margin instead, its text
rewrapped, and a dialog taller than the window scrolls its body under
the title and above its buttons (`ui::modal_frame`, so it holds for all
of them).

**Gallery sidebar.** On a phone-width window the gallery's sidebar (view,
grouping, folders, buckets, people) is a drawer over the grid's left
edge rather than a column beside it: a rightward swipe across the grid
opens it, a leftward swipe across the drawer or the grid closes it, as
does a tap on the grid, and the folder button at the start of the
gallery's strip toggles it. The swipe is read from the finger's travel as
gpui's scroll events report it, so the grid keeps scrolling vertically
and only a mostly sideways travel counts. So that a swipe never selects
or opens what it starts on, the gallery's rows, links and tiles act on
the finger lifting rather than landing on iOS (`PressExt::on_press` in
`gallery_chrome.rs`; the desktop keeps acting on the press so a drag
carries the selection): gpui's backend cancels a press the moment the
finger moves, so only a tap reaches them.

**Side panels.** The navigator, colour, layers and history column is
320pt wide on iOS and folds away with the button at the right end of the
tool options bar (also `ToggleSidePanels`); the choice persists with the
view preferences. On a phone-width window there is no room for both, so
the same button switches the body between the canvas (with its toolbar)
and the panel column, its icon naming the one it switches to; that
choice is per session and starts on the canvas.

**Compiled out.** Everything a sandboxed app cannot host, gated by the
`sandboxed` cfg that `crates/app/build.rs` sets for iOS and the browser:

- The Photoshop plug-in host (helper processes) and the WebAssembly
  plug-in host (a JIT). The first-party plugins are all there.
- The AI sidebar, which drives locally installed agent CLIs, and the MCP
  bridge and gallery tools that serve it.
- The self-updater: the store updates the app.
- Dragging photos out to a file manager.
- Quit in the menus: iOS apps do not quit themselves.

Both menu sets, the editor's and the gallery's, are pruned the same way,
and Preferences stays in the View menu on iOS, where there is no
application menu a finger can reach (macOS keeps it in the app menu).

Unlike the browser build, iOS keeps the gallery, the GPU compositor, the
crash reporter, the daily ping, font and model downloads, and the cloud.

**GPU compositing** runs on iOS as on the desktop: `wgpu` opens the
Metal adapter (the app's own instance; gpui's renderer is separate) and
the layer stack, adjustments, blend modes, filters and warps run as the
same compute kernels. The compositor's parity tests, which check every
kernel's output against the CPU reference, pass on the Simulator's
Metal GPU: `tools/ios-test.sh -p schist-compositor-gpu` builds a crate's
tests for the Simulator, wraps each binary in a bundle and runs it there
with its output on the terminal. The Simulator's GPU lacks a few WebGPU
downlevel flags (indirect execution, base vertex, cube arrays,
comparison samplers); `wgpu` warns about them at start-up and the
compositor uses none. When the adapter or device cannot be opened, the
app logs why and stays on the CPU, and the Preferences toggle turns the
GPU off. gpui pauses drawing while the app is in the background, and
the compositor only runs from drawing and from edits, so no Metal work
is submitted from the background, which iOS would terminate the app
for.

**Native on iOS.** Two things the desktop borrows from elsewhere, iOS has
built in:

- HEIC decodes through ImageIO (`plugins/codecs-common/src/heif_imageio.rs`)
  instead of the downloaded libheif: the container's rotation and crop are
  applied, the image is drawn once into RGBA in its own colour space with
  its ICC profile carried over, and an EXIF orientation is honoured. The
  libheif download dialog never appears. Import only, as on the desktop;
  HDR (PQ/HLG) captures come out as the system's SDR rendering rather than
  the desktop's own tone mapping.
- "Save to Photos", in the File menu and as the arrow button at the
  right end of the tool options bar, encodes the flattened document as
  PNG (as Export does) and adds it to the camera roll through the photo
  library (`crates/app/src/workspace/photos_save.rs`). The first save
  asks for add-only permission; the outcome lands in the status bar.
- A file another app hands over (the share sheet's "Copy to Schist",
  Files' "Open in Schist", a photo sent from Photos) is asked about:
  add it to the gallery, or open it in the editor? The gallery copies it
  into `Documents/Photos` and shows it; the editor copies it into
  `Documents` and opens it. A file already under Documents stays put,
  and one from the sandbox's Inbox is removed once copied, as iOS
  expects (`crates/app/src/workspace/shared_files.rs`). The desktop
  opens handed-over files outright, as before.
- The gallery's "Import from Photos…" (the desktop's "Import from
  Camera…") opens the system photo picker (`PHPickerViewController`,
  `crates/app/src/workspace/library_photos.rs`), which needs no photo
  library permission. What is picked is copied as the original files,
  HEIC included, into the app's `Documents/Photos`, which joins the
  gallery's watched folders; from there thumbnails, EXIF, edits and
  sidecars work as for any folder, and the folder is visible in the Files
  app. iOS moves the app's container on every install, so `library.json`
  stores in-container paths relative to it (`$SANDBOX`), and a watched
  folder that no longer exists is dropped at launch.

**Cloud sign-in.** The cloud's login lives in the keychain, which an iOS
app can only use when it is signed with `keychain-access-groups`. The
unsigned Simulator bundle `tools/ios-build.sh` makes has no keychain:
the app starts with no login rather than an error, and signing in
reports that the login cannot be kept. A signed build (a device build,
or a Simulator run from an Xcode project) keeps it as on the desktop.

## Status

Compiles and type-checks for the Simulator and device targets. Launch,
rendering, and touch in the Simulator are checked by hand with
`tools/ios-build.sh --debug`; see the gpui fork's docs for what its
backend has and has not been verified on.
