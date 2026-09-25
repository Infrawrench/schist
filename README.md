<img src="assets/logo/schist-512.png" alt="" width="88" align="right">

# Schist

A layered image editor and photo manager written in Rust on [GPUI].
Paint, retouch, develop camera RAW files, work with layered documents, and
organize a photo library in the same app. Tools, filters, formats and menu
commands share a plugin registry with the headless editor and automation APIs.

Schist runs on Linux, macOS and Windows, with iOS, Android and browser builds.
See the platform guides below for their requirements and differences.

[Download desktop releases](https://github.com/Infrawrench/schist/releases)
· [Try it in your browser](https://try.schist.app)
· [Documentation](#documentation)

[GPUI]: https://gpui.rs

## Backers

<!-- backers:start -->

### Gold

<a href="https://leanercloud.com"><picture><source media="(prefers-color-scheme: dark)" srcset="https://backers.schist.app/api/logos/1070272e-84cb-4a8d-8b8d-3976fbf81d88/dark"><img src="https://backers.schist.app/api/logos/1070272e-84cb-4a8d-8b8d-3976fbf81d88/light" alt="LeanerCloud" width="120"></picture></a>
<!-- backers:end -->


Thanks to the people and brands supporting Schist.

The app's **💜 Support Schist** dialog embeds [backers.json](backers.json).
The backer updater can replace this file: `support_url` is the support page,
and `tiers` is an ordered list of `{name, backers}` groups. Each backer has a
`name`, an optional website `url`, and an optional `logo` with a `light` URL,
an optional `dark` URL, and a display `width` (defaults to 120 pixels).
Tier and backer names are displayed as supplied; empty tiers are hidden.

The editor's `build.rs` downloads the logos and embeds them, so this dialog
works offline on every platform. Downloads are cached by URL in Cargo's
build output; a clean build needs network access. Use versioned logo URLs
when artwork changes, or `SCHIST_REFRESH_BACKER_LOGOS=1 make app` to refresh
the cache. No backer or logo requests are made while the app runs.

## Build and run

Install stable Rust through `rustup`, GNU Make, and your platform's native
build toolchain. The repository's [rust-toolchain.toml](rust-toolchain.toml)
selects the Rust channel and components.

On Debian/Ubuntu, install the Linux build dependencies:

```sh
sudo apt-get install build-essential pkg-config libfontconfig-dev \
  libwayland-dev libxkbcommon-x11-dev libxcb1-dev libxcb-render0-dev \
  libxcb-shape0-dev libxcb-xfixes0-dev libvulkan-dev libgphoto2-dev libclang-dev clang mold
```

Linux also needs a working Vulkan driver at runtime. For Mesa drivers on
Debian/Ubuntu, install `mesa-vulkan-drivers`; other GPUs may need their
vendor's driver. The Vulkan loader alone is not a driver.

From a checkout, build and launch the desktop editor:

```sh
make app
./target/release/schist
# Or open a document:
./target/release/schist path/to/image.psd
```

On Windows, run `target/release/schist.exe`. For development, use
`make app PROFILE=debug` and the executable under `target/debug/`.

`make app` builds the editor without the Photoshop filter helper bundle.
`make build` includes those helpers; on Linux their cross-compilation also
needs `gcc-mingw-w64`. See the [helper build guide](docs/8bf-host.md#building-the-helpers)
for platform requirements, and `make help` for the main build targets.

| Target | Output / guide |
| --- | --- |
| `make release` | Platform packages in `dist/`; [release and signing details](docs/versioning.md) |
| `make web` | Static browser app in `dist/web/`; [build and serve locally](docs/web.md) |
| `make ios PROFILE=debug` | Simulator bundle in `dist/ios/`; [iOS and iPadOS](docs/ios.md) |
| `make ios-device` | Device bundle, requiring signing; [device setup](docs/ios.md#building-and-running) |
| `make android PROFILE=debug` | APK in `dist/android/`; [Android SDK and installation](docs/android.md) |
| `make library` | Native C API and browser/Node WebAssembly bindings in `dist/library/`; [library guide](docs/library.md) |

The browser needs WebGPU and a secure context (`localhost` or HTTPS).
Mobile builds need their platform SDKs. These builds share the editor, with
platform-specific file access, plugin and background-service support described
in their guides.

The interface follows the system's preferred language. The
[locale registry](crates/i18n/locales.tsv) lists the shipped translations;
`SCHIST_LANG=de` overrides the language for a native run. See
[internationalisation](docs/i18n.md) for translation and font support.

## What it does

**Layered editing.** Layers and groups, masks and clipping masks, blend modes,
adjustment layers, layer effects, artboards, slices and layer comps. Work with
[embedded and linked smart objects](docs/smart-objects.md) and
[editable filter stacks](docs/filter-stacks.md) that retain their source pixels
through supported moves and transforms. History, undo/redo and native crash
recovery keep edits recoverable.

**Painting and retouching.** Brush, pencil, erasers, gradients, clone and healing
tools, patching, content-aware fill and move, dodge/burn, blur, sharpen and smudge.
[Brush presets](docs/brushes.md) support imported bitmap tips and brush packs,
spacing, scattering, stroke stabilization, and pressure-controlled size and
opacity. [Symmetry and seamless painting](docs/symmetry-painting.md) add mirrored
or radial strokes and wraparound texture painting. Stylus tilt is supported in
the browser where the device supplies it; native tilt is not yet available.

**Selections and masks.** Marquees, lassos, wand, quick and object selection,
colour range, grow/similar, feathering and saved selections.
[Mask refinement](docs/mask-refinement.md) provides edge cleanup and previews
before applying the result to a selection or layer mask.

**Vectors and text.** Editable paths and live shapes, path selection, fills and
strokes. [Text layers](docs/text.md) support OpenType controls, text on paths,
mixed-direction paragraphs and vertical writing. Editable interchange depends
on the destination format; see [file formats](#file-formats).

**Filters and transforms.** Blur, sharpen, noise, distortion, artistic and neural
filters, Camera Raw, [Lens Correction](docs/lens-profiles.md) and Filter Gallery.
Lens Correction can match EXIF camera/lens metadata to installed or imported
Lensfun calibration, with profile overrides and portable saved coefficients.
Filters preview on the
canvas, with [draggable controls](docs/filter-canvas.md) for supported blur and
lighting effects. Free Transform, Liquify, Puppet Warp, Content-Aware Scale and
Vanishing Point cover geometric edits.

**Colour.** ICC profiles, assign/convert operations, display transforms and soft
proofing. [Native CMYK and Lab editing](docs/native-colour-editing.md) preserves
process channels through supported edits and saves. Import `.aco`, `.ase` and
`.acb` palettes in the Color panel, including user-supplied colour books; no
Pantone libraries are bundled. [Spot ink channels](docs/spot-ink.md) provide
editable separations and an overprint display simulation, with PSD/PSB
interchange and documented proofing limits.

**Photo library.** The [gallery](docs/gallery.md) watches local folders, imports
from cameras, organizes photos by folder/date/place, and provides buckets, maps,
search and People indexing. Rate, flag and label photos, compare them with
synchronized zoom and pan, and review similar images or capture bursts. Gallery
edits use sidecars and version history so the originals remain available.
[Metadata editing](docs/photo-metadata.md) supports individual or batch changes
to keywords, captions, copyright, capture times and GPS through portable XMP
sidecars.

**Photo workflows.** [Merge photos](docs/photo-merging.md) into aligned layers,
focus stacks, bracketed HDR images or translation-based panoramas on native
builds. [Recorded actions](docs/actions.md) replay editing steps, and
[export recipes](docs/export-recipes.md) save reusable sets of outputs for
documents and gallery selections.

**Video frames.** The native [video viewer](docs/gallery.md#video) provides silent
playback, frame stepping, nearby sharper-frame search, and capture into an image
document. Codec availability depends on the platform; Linux uses system
GStreamer plugins. Audio and video editing can be handed to an external editor.

**Schist Cloud.** Sign in from the welcome screen or File menu to browse remote
folders and buckets, upload photos, search, and edit collaboratively. Compatible
providers also support metadata, photo review, version history, and syncing
saved brushes, actions and export recipes. Mobile camera-roll backup is opt-in.
Cloud is enabled by default; [feature flags](docs/feature-flags.md) can disable
it. See the [Cloud guide](docs/cloud.md) for sign-in, transfers and platform
support, and the [gallery sync implementation notes](docs/cloud-parity-2026-09-20.md)
for provider-dependent workflows.

**Workspace and search.** Spotlight (`Cmd/Ctrl+Shift+P`) finds tools, commands,
filters, documents, layers and photos. The editor includes draggable and
resizable side panels, rulers, guides, snapping, a navigator, themes and
remappable keyboard shortcuts.

**GPU acceleration.** Supported compositing, viewport rendering, filters and
other pixel operations use GPU compute, with CPU fallbacks for unsupported
operations or unavailable compute adapters. Browser canvas and supported
filters use asynchronous WebGPU. Preferences and `SCHIST_GPU=0` / `SCHIST_GPU=1`
control native GPU use; see [browser GPU support](docs/web.md) for web-specific
boundaries.

## File formats

| Format | Support |
| --- | --- |
| PSD / PSB | Layered 8/16/32-bit read/write with groups, masks, blend modes, adjustments, effects, native colour channels and spot separations. Supported text and smart filters remain editable in other readers; see [PSD interchange](docs/psd-interchange.md) and [native smart filters](docs/native-smart-filters.md). |
| Affinity `.af`, `.afphoto`, `.afdesign`, `.afpub` | Import layered documents; export layered `.af` files, including supported native text and curves. Unsupported content may use preserved native data or raster previews. See [Affinity support and limits](docs/affinity-format.md). |
| Paint.NET `.pdn` / GIMP `.xcf` | Read/write supported layered content; see [format limits](docs/layered-formats.md). |
| PNG, JPEG, WebP, TIFF | Import and export raster images. |
| HEIC / HEIF | Native import through a supported runtime libheif decoder; Schist can offer a download when needed. |
| Camera RAW | Import through Schist's pure-Rust decoder and develop in Camera Raw. The original capture and development settings can survive PSD/PSB save and reopen. See [camera and codec coverage](crates/codec-raw/README.md). |

Format support is not a guarantee of identical rendering or complete feature
interchange. Schist preserves unrecognized PSD data where supported, and uses
its own blocks for editing state that other applications may not understand or
retain. The format guides describe editable subsets, raster fallbacks and known
limits.

Packaged macOS builds include [Quick Look extensions](docs/quicklook.md) for
thumbnails and previews of supported layered documents.

## Keyboard

Photoshop's defaults (⌘ on macOS, Ctrl elsewhere):

| Area | Shortcuts |
| --- | --- |
| Tools | `V` move · `M` marquee · `L` lasso · `W` wand · `C` crop · `B` brush · `E` eraser · `S` clone · `J` spot healing · `Y` history brush · `G` gradient · `O` dodge · `P` pen · `A` path selection · `T` type · `U` shapes · `I` eyedropper · `H`/space hand · `Z` zoom |
| Tool groups | Shift+the tool's key cycles nested tools (Shift+`M` marquee ⇄ ellipse); hold or right-click a toolbar slot for its flyout |
| Edit | ⌘Z / ⌘⇧Z undo・redo · ⌘X/C/V · ⌘⇧C copy merged · ⌘T free transform |
| Select | ⌘A all · ⌘D deselect · ⌘⇧D reselect · ⌘⇧I inverse · shift/alt-drag to add/subtract |
| Layers | ⌘⇧N new · ⌘J duplicate · ⌘⇧J via cut · ⌘G group · ⌘E/⌘⇧E merge · ⌘[ ⌘] reorder · ⌘⌥G clipping mask |
| Adjust | ⌘L levels · ⌘M curves · ⌘U hue/sat · ⌘I invert |
| Fill | ⇧F5 Fill… · ⌥⌫ / ⌃⌫ fill with fore/background |
| View | ⌘0 fit · ⌘1 100% · ⌘R rulers · ⌘' grid · ⌘; guides · ⌘H extras · Tab/F screen modes · ⌘K preferences |
| Painting | `[`/`]` brush size · digits set opacity · `D`/`X` default・swap colours |

## Mouse and touchpad

Two-finger scroll pans; **Ctrl/Cmd/Alt + scroll zooms** toward the pointer.
**Preferences → Zoom with scroll wheel** swaps these behaviours. Pinch support
and stylus input depend on the platform; Windows precision touchpads normally
zoom through Ctrl+scroll. The mobile guides describe touch and pen controls.

Remap shortcuts in `~/.config/schist/keymap.json` (or under
`$XDG_CONFIG_HOME/schist/`):

```json
{ "ctrl-shift-x": "command:edit.fill_foreground", "f1": "tool:brush" }
```

## Plugins and automation

The desktop app loads sandboxed WebAssembly filters and codecs. Plugins have
no filesystem, network or clock access, and run with a fuel budget. Put `.wasm`
files in `~/.config/schist/plugins/`, or manage them with **File → Plugins…**.
The [plugin guide](docs/plugin-guide.md) covers the SDK and ABI, with
[example plugins](examples/plugins) to build on. Photoshop filter hosting uses
separate native helper processes; see the [host guide](docs/8bf-host.md) for
compatibility and setup.

The [MCP server](docs/mcp.md), `schist-mcp`, exposes headless editing sessions,
tools, filters, commands, gallery access and rendered previews to MCP clients.
It ships alongside desktop releases. For embedding, the
[headless library](docs/library.md) provides a C API and WebAssembly bindings
with independently owned editor instances.

The desktop [AI panel](docs/ai-panel.md), under **View → AI Panel**, uses an
installed and authenticated `claude` or `codex` CLI to work with the open
document or gallery. Document edits appear as undoable history entries.

## Diagnostics

Native builds send a daily usage ping to `telemetry.schist.app` by default.
It contains a random installation ID, app version, OS and architecture, CPU
model and core count, GPU adapter/driver, and RAM capacity. It does not include
usernames, hostnames, file paths or document contents.

The server keeps the latest ping per ID, its first and last contact times,
and the country Cloudflare derives from the request. The IP address itself
is not stored.

Disable it in **Preferences → Diagnostics**, set `SCHIST_NO_TELEMETRY=1`, or
create an empty `no_telemetry` file in `~/.config/schist/` (or
`$XDG_CONFIG_HOME/schist/`).

Update checks and crash reporting are opt-in. The Diagnostics preferences
separately control local crash reports and uploads to the project's Sentry.
Uploads require a build configured with a reporting endpoint; ordinary source
builds have none. Crash uploads omit hostnames and breadcrumbs and redact the
home directory from paths. `SCHIST_CRASH_REPORTS=1` and `SCHIST_CRASH_UPLOAD=1`
enable the respective options for a single run. Cloud, map tiles, font/model
downloads and agent integrations make network requests when those features are
used.

## Development

Use the Makefile for builds and the checks relevant to the area you change:

```sh
make check-app       # type-check application crates and their tests
make test-app        # test application crates
make lint-app        # lint application crates
make check-app-web   # type-check the browser application
make check-i18n      # validate translations and the browser loader's strings
```

These application checks are a subset of the workspace checks. The
[CI workflow](.github/workflows/ci.yml) defines workspace formatting, lint,
tests and platform checks; individual feature guides document more focused
Make targets and any hardware or fixture requirements.

Read [AGENTS.md](AGENTS.md) for repository guidance. Rust interface strings go
through `crates/i18n`; browser loading messages use the web i18n support.
[Architecture](docs/architecture.md) describes the crate boundaries, and
[versioning](docs/versioning.md) covers compatibility and releases.

The logo and platform icons are generated from [tools/logo.py](tools/logo.py).
Edit that source and run `make logos` (requires Pillow) to regenerate them.

## Documentation

- **Platforms:** [Browser](docs/web.md), [iOS/iPadOS](docs/ios.md),
  [Android](docs/android.md), [macOS Quick Look](docs/quicklook.md).
- **Library and workflows:** [Gallery](docs/gallery.md),
  [photo metadata](docs/photo-metadata.md), [photo merging](docs/photo-merging.md),
  [actions](docs/actions.md), [export recipes](docs/export-recipes.md),
  [Cloud](docs/cloud.md).
- **Editing:** [Brushes](docs/brushes.md), [symmetry](docs/symmetry-painting.md),
  [mask refinement](docs/mask-refinement.md), [text](docs/text.md),
  [smart objects](docs/smart-objects.md), [filter stacks](docs/filter-stacks.md),
  [filter canvas controls](docs/filter-canvas.md),
  [native colour](docs/native-colour-editing.md), [spot ink](docs/spot-ink.md).
- **Interchange:** [PSD/PSB](docs/psd-interchange.md),
  [native smart filters](docs/native-smart-filters.md),
  [Affinity](docs/affinity-format.md), [PDN/XCF](docs/layered-formats.md),
  [RAW](crates/codec-raw/README.md).
- **Extending Schist:** [Architecture](docs/architecture.md),
  [plugins](docs/plugin-guide.md), [headless library](docs/library.md),
  [MCP](docs/mcp.md), [AI panel](docs/ai-panel.md),
  [shared document engine](docs/document-library.md),
  [internationalisation](docs/i18n.md), [feature flags](docs/feature-flags.md),
  [versioning](docs/versioning.md).

Schist is available under the [MIT license](LICENSE).
