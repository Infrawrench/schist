# Schist build orchestration.
#
# `cargo build` builds the app and is still the thing to reach for. This
# file exists for the one job cargo cannot do on its own: the Photoshop
# plug-in helpers.
#
# A `.8bf` plug-in is a binary for a particular OS and architecture, and
# it is very often not the one Schist was compiled for — a 32-bit Windows
# filter on 64-bit Linux, an Intel filter on an Apple Silicon Mac. Schist
# runs each one in a helper process built for the *plug-in's* target, so
# every install needs several helpers alongside a single app binary.
# Cargo builds one target per invocation, so something has to drive it
# once per architecture. That is all this is.
#
#   make helpers                  the helpers this platform can use
#   make helpers PROFILE=debug    ... beside a debug build
#   make install-helpers DESTDIR=path/to/somewhere
#   make all                      app and helpers
#
# Deliberately *not* a build.rs: a build script that shells out to cargo
# re-enters a cargo that already holds the lock on target/, and blocks
# until it times out.

CARGO   ?= cargo
RUSTUP  ?= rustup
PROFILE ?= release

HELPER_CRATE := schist-plugin-host-8bf
HELPER_BIN   := schist-8bf-helper

# Helpers always build with the `helper` profile, whatever the app is
# built with: they are a shipping artifact either way, and the profile
# strips them. Debug info is 86% of a helper's size and nothing reads it
# -- see the profile's comment in Cargo.toml.
HELPER_PROFILE := helper

# `--release` names the directory `release`; the default profile builds
# into `debug` and takes no flag at all.
ifeq ($(PROFILE),release)
  PROFILE_FLAG := --release
else
  PROFILE_FLAG :=
endif

DESTDIR ?= target/$(PROFILE)

# Where `make build` stages the helpers for the app build to embed. An
# absolute path: build.rs turns these into `include_bytes!` arguments,
# and cargo runs it from the crate directory rather than this one.
HELPER_STAGE := $(CURDIR)/target/helper-bundle/$(PROFILE)

# Only used to name the Windows installer, and only read on Windows.
VERSION := $(shell sed -n '0,/^version = /s/^version = "\(.*\)"/\1/p' Cargo.toml)

# Windows sets OS in the environment and may have no `uname` at all, so
# it is checked first; MSYS and Cygwin set it too and are Windows for
# this purpose. Anything unrecognised is an error rather than a guess —
# defaulting would silently build the wrong architectures.
ifeq ($(OS),Windows_NT)
  HOST := windows
else
  UNAME_S := $(shell uname -s)
  ifeq ($(UNAME_S),Linux)
    HOST := linux
  else ifeq ($(UNAME_S),Darwin)
    HOST := macos
  else
    HOST := unknown
  endif
endif

# Which plug-ins this platform can host, from the table in
# `crates/plugin-host-8bf/src/launch.rs`. Linux and Windows both host
# Windows plug-ins — Linux by way of Wine, which runs the same PE binary
# — so both build a pair of `.exe` helpers, differing only in whether
# they link against mingw or MSVC.
ifeq ($(HOST),linux)
  HELPER_TARGETS := x86_64-pc-windows-gnu i686-pc-windows-gnu
else ifeq ($(HOST),macos)
  HELPER_TARGETS := aarch64-apple-darwin x86_64-apple-darwin
else ifeq ($(HOST),windows)
  HELPER_TARGETS := x86_64-pc-windows-msvc i686-pc-windows-msvc
else
  HELPER_TARGETS :=
endif

# What each helper is called once installed. These names are not
# decoration: `Helper::file_name` looks a helper up by this exact string,
# so changing one here is a runtime failure rather than a build one.
# `tests/launch.rs` pins the same names from the Rust side.
name-x86_64-pc-windows-gnu  := schist-8bf-helper-x86_64.exe
name-i686-pc-windows-gnu    := schist-8bf-helper-x86.exe
# These two are also spelled out in .github/workflows/release.yml, whose
# Windows job stages helpers without make: MSYS make would hand build.rs
# an /d/a/... path that a native Windows build cannot read. Both sides are
# pinned from Rust by `Helper::file_name` in tests/launch.rs.
name-x86_64-pc-windows-msvc := schist-8bf-helper-x86_64.exe
name-i686-pc-windows-msvc   := schist-8bf-helper-x86.exe
name-x86_64-apple-darwin    := schist-8bf-helper-x86_64
name-aarch64-apple-darwin   := schist-8bf-helper-arm64

# Cargo's own output name differs from the installed one only by the
# extension, which Windows targets carry and Unix ones do not.
exe = $(if $(findstring windows,$(1)),.exe,)

HELPERS := $(foreach t,$(HELPER_TARGETS),$(DESTDIR)/$(name-$(t)))

.DEFAULT_GOAL := help
.PHONY: help all app build web android helpers install-helpers preflight release check-bundle clean-helpers FORCE

help:
	@echo 'make build            the app, carrying the plug-in helpers ($(PROFILE))'
	@echo 'make release          build and package into dist/'
	@echo
	@echo 'make app              just the Schist binary, no helpers'
	@echo 'make check-app        type-check all application crates and their tests'
	@echo 'make test-app         test all application crates'
	@echo 'make lint-app         lint all application crates'
	@echo 'make check-app-web    type-check the browser application'
	@echo 'make library          headless shared library + WASM, into dist/library/'
	@echo 'make web              the browser build, into dist/web/'
	@echo 'make android          the Android package, into dist/android/'
	@echo 'make logos            regenerate the logo and platform app icons (Pillow)'
	@echo 'make helpers          just the .8bf plug-in helpers, beside the binary'
	@echo 'make install-helpers DESTDIR=DIR   put the helpers somewhere else'
	@echo
	@echo 'this platform hosts plug-ins built for:'
	@$(foreach t,$(HELPER_TARGETS),echo '  $(t)  ->  $(name-$(t))';)

all: build

# The app, carrying the helpers inside it.
#
# They are staged somewhere of their own rather than reused from beside
# the binary, so that what gets embedded is exactly what this build
# produced and not whatever an earlier `make helpers` left lying there.
build: stage-helpers
	SCHIST_BUNDLED_HELPERS='$(HELPER_STAGE)' $(CARGO) build $(PROFILE_FLAG) -p schist-app

.PHONY: stage-helpers
stage-helpers:
	@$(MAKE) --no-print-directory install-helpers \
	  DESTDIR='$(HELPER_STAGE)' PROFILE='$(PROFILE)'

# Just the binary. Without helpers it still runs; it just has none to
# unpack, and says so if asked to run a plug-in.
app:
	$(CARGO) build $(PROFILE_FLAG) -p schist-app

# Check and test the launcher plus every crate extracted from the app.
APP_CRATES := app editor app-actions app-ai app-fonts app-platform app-services \
              app-settings camera-sync tethered cloud-transfer gallery-ui map-view video
APP_PACKAGES := $(foreach crate,$(APP_CRATES),-p schist-$(crate))
.PHONY: check-app test-app lint-app check-app-web
check-app:
	$(CARGO) check $(APP_PACKAGES) --all-targets

.PHONY: check-photo-merge test-photo-merge fmt-photo-merge lint-photo-merge
check-photo-merge:
	$(CARGO) check -p schist-photo-merge -p schist-editor --all-targets
lint-photo-merge:
	$(CARGO) clippy -p schist-photo-merge -p schist-editor -p schist-app-actions --all-targets -- -D warnings
test-photo-merge:
	$(CARGO) test -p schist-photo-merge
.PHONY: test-photo-merge-editor
test-photo-merge-editor:
	$(CARGO) test -p schist-editor photo_merge::tests
	$(CARGO) test -p schist-tools-paint layer_mask_brush
fmt-photo-merge:
	$(CARGO) fmt -p schist-photo-merge -p schist-editor -p schist-app-actions
test-app:
	$(CARGO) test $(APP_PACKAGES)
lint-app:
	$(CARGO) clippy $(APP_PACKAGES) --all-targets -- -D warnings
check-app-web:
	$(CARGO) check -p schist-app --target wasm32-unknown-unknown

.PHONY: test-new-doc-clipboard
test-new-doc-clipboard:
	$(CARGO) test -p schist-editor --lib workspace::clipboard::tests

.PHONY: test-brush-workflows check-brush-workflows
test-brush-workflows:
	$(CARGO) test -p schist-tools-paint -p schist-app-settings
check-brush-workflows:
	$(CARGO) check -p schist-editor --all-targets

# The browser deployment, assembled into dist/web/. A script rather than
# rules here: it is one linear pipeline (bindgen, opt, chunk, manifest)
# with nothing make's dependency graph would add. See docs/web.md.
web:
	./tools/web-build.sh

# The Android package, assembled into dist/android/Schist.apk. A script
# for the same reason as the web build; without --no-run it also installs
# and launches the app on a device or emulator. See docs/android.md.
android:
	./tools/android-build.sh $(if $(filter debug,$(PROFILE)),--debug,) --no-run

.PHONY: logos
logos:
	python3 tools/logo.py

.PHONY: ios ios-device check-camera-sync check-camera-sync-ios check-camera-sync-android
ios:
	./tools/ios-build.sh $(if $(filter debug,$(PROFILE)),--debug,) --no-run
ios-device:
	./tools/ios-build.sh $(if $(filter debug,$(PROFILE)),--debug,) --device
check-camera-sync:
	$(CARGO) test -p schist-camera-sync
check-camera-sync-ios:
	$(CARGO) check -p schist-app --target aarch64-apple-ios
check-camera-sync-android:
	./tools/android-build.sh --check

.PHONY: check-i18n check-i18n-wasm
check-i18n:
	$(CARGO) test -p schist-i18n
	python3 tools/check-i18n.py
	python3 tools/sync-i18n.py --check
	node --test web/i18n.test.mjs

check-i18n-wasm:
	$(CARGO) check -p schist-i18n --target wasm32-unknown-unknown

helpers: preflight $(HELPERS)

install-helpers: helpers

# Linking a Windows binary from Linux needs mingw's linker, and rustc's
# failure when it is absent names only `cc`, which is present and is not
# the problem. Say so plainly instead.
preflight:
ifeq ($(HOST),unknown)
	@echo 'error: unrecognised host "$(UNAME_S)"; no idea which helpers to build.' >&2; exit 1
endif
ifeq ($(HOST),linux)
	@command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1 || { \
	  echo 'error: the Windows plug-in helpers need mingw-w64 to link.'; \
	  echo '       Debian/Ubuntu: sudo apt install gcc-mingw-w64'; \
	  echo '       Fedora:        sudo dnf install mingw64-gcc mingw32-gcc'; \
	  echo '       Arch:          sudo pacman -S mingw-w64-gcc'; \
	  exit 1; }
endif

$(DESTDIR):
	@mkdir -p $@

# One rule per architecture, generated rather than written out, so the
# list above stays the only place a target is named.
#
# The recipe depends on FORCE and not on the crate's sources: cargo is
# the incremental build system here, and it already knows what changed.
# Restating its dependency graph in make would only be a second, worse
# copy of it — and a stale one the first time a file is added.
define helper_rule
$$(DESTDIR)/$$(name-$(1)): FORCE | $$(DESTDIR)
	@$$(RUSTUP) target list --installed 2>/dev/null | grep -qx '$(1)' \
	  || $$(RUSTUP) target add $(1)
	SCHIST_BUNDLED_HELPERS= $$(CARGO) build --profile $$(HELPER_PROFILE) \
	  -p $$(HELPER_CRATE) --bin $$(HELPER_BIN) --target $(1)
	@cp target/$(1)/$$(HELPER_PROFILE)/$$(HELPER_BIN)$$(call exe,$(1)) $$@
	@echo '  helper   $$@' $$$$(du -h '$$@' 2>/dev/null | cut -f1)
endef
$(foreach t,$(HELPER_TARGETS),$(eval $(call helper_rule,$(t))))

# Packaging. Each script runs its own cargo build, so the staged helpers
# are exported rather than passed: the build inside picks them up and the
# packaged binary carries them, with no change to the scripts themselves.
release: stage-helpers
	@mkdir -p dist
ifeq ($(HOST),linux)
	SCHIST_BUNDLED_HELPERS='$(HELPER_STAGE)' ./packaging/linux/appimage.sh
	SCHIST_BUNDLED_HELPERS='$(HELPER_STAGE)' ./packaging/linux/packages.sh
else ifeq ($(HOST),macos)
	SCHIST_BUNDLED_HELPERS='$(HELPER_STAGE)' ./packaging/macos/bundle.sh $(PROFILE)
else ifeq ($(HOST),windows)
	SCHIST_BUNDLED_HELPERS='$(HELPER_STAGE)' $(CARGO) build $(PROFILE_FLAG) -p schist-app -p schist-mcp
	cp target/$(PROFILE)/schist.exe target/$(PROFILE)/schist-mcp.exe dist/
	makensis -DVERSION=$(VERSION) packaging/windows/installer.nsi
else
	@echo 'error: nothing to package on this host' >&2; exit 1
endif
	@echo 'packaged into dist/'

# Build the helpers, embed them, and check they unpack. The unpacking
# tests only have something to unpack when a bundle is present, so this
# is the run that exercises them at all.
check-bundle: stage-helpers
	SCHIST_BUNDLED_HELPERS='$(HELPER_STAGE)' \
	  $(CARGO) test -p $(HELPER_CRATE) --lib bundled

clean-helpers:
	rm -f $(HELPERS)
	rm -rf $(HELPER_STAGE)

FORCE:

.PHONY: check-recordable-actions check-recordable-actions-catalogs format-recordable-actions
check-recordable-actions:
	$(CARGO) test -p schist-editor --lib workspace::recorded_actions::tests
	$(CARGO) check -p schist-editor --all-targets
.PHONY: check-adjustment-refresh
check-adjustment-refresh:
	$(CARGO) test -p schist-core --lib
	$(CARGO) test -p schist-compositor --lib

.PHONY: check-layer-drag bench-layer-drag
check-layer-drag:
	$(CARGO) test -p schist-compositor -p schist-tools-basic
	$(CARGO) check -p schist-editor --all-targets
bench-layer-drag:
	$(CARGO) run $(PROFILE_FLAG) -p schist-compositor --example bench_drag

.PHONY: bench-canvas check-canvas
bench-canvas:
	$(CARGO) run $(PROFILE_FLAG) -p schist-compositor-gpu --example canvasbench
.PHONY: bench-layer-transform check-layer-transform
bench-layer-transform:
	$(CARGO) run $(PROFILE_FLAG) -p schist-compositor-gpu --example transformbench
check-layer-transform:
	$(CARGO) test -p schist-core -p schist-tools-transform
	$(CARGO) check -p schist-editor --all-targets
check-canvas:
	$(CARGO) test -p schist-compositor -p schist-compositor-gpu --lib
	$(CARGO) test -p schist-compositor-gpu --test parity --test native_parity --test tile_uploads
	$(CARGO) check -p schist-editor --all-targets

format-recordable-actions:
	$(CARGO) fmt -p schist-editor -p schist-app-actions
check-recordable-actions-catalogs:
	python3 tools/check-i18n.py
	python3 tools/sync-i18n.py --check

.PHONY: check-layered-codecs check-layered-codecs-wasm check-layered-codecs-app
# HEIC security gate, download upgrade, and real-image import regressions.
# Serial execution keeps the install test's managed-directory override
# from changing where the import tests find their decoder.
# SCHIST_REQUIRE_HEIF=1 requires a decoder and forbids skipped imports.
.PHONY: check-heif
check-heif:
	$(CARGO) test -p schist-codecs-common heif -- --test-threads=1 --nocapture

check-layered-codecs:
	$(CARGO) test -p schist-codecs-common -p schist-preview
	$(CARGO) clippy -p schist-codecs-common -p schist-preview --all-targets -- -D warnings

check-layered-codecs-wasm:
	$(CARGO) check -p schist-codecs-common -p schist-preview --target wasm32-unknown-unknown

check-layered-codecs-app:
	$(CARGO) check -p schist-app

# README's text layout and GPU effect work.
.PHONY: check-text check-gpu-fx check-readme
check-text:
	$(CARGO) test -p schist-core -p schist-text-engine -p schist-tools-type -p schist-codec-affinity

.PHONY: check-psd-interchange lint-psd-interchange
check-psd-interchange:
	$(CARGO) test -p schist-psd-descriptor -p schist-codec-psd
lint-psd-interchange:
	$(CARGO) clippy -p schist-psd-descriptor -p schist-codec-psd -p schist-text-engine --all-targets -- -D warnings

.PHONY: check-psd-effects
check-psd-effects:
	$(CARGO) test -p schist-codec-psd -p schist-layer-fx
	$(CARGO) test -p schist-compositor-gpu --test compute_programs
	$(CARGO) check -p schist-editor --all-targets

.PHONY: check-psd-light
check-psd-light:
	$(CARGO) test -p schist-adjustments -p schist-codec-psd -p schist-app-actions
	$(CARGO) test -p schist-compositor-gpu --test adjustment_coverage --test compute_programs
	$(CARGO) check -p schist-editor --all-targets

.PHONY: check-editable-interchange lint-editable-interchange inspect-affinity-interchange fmt-editable-interchange
check-editable-interchange:
	$(CARGO) test -p schist-codec-affinity -p schist-codec-psd -p schist-text-engine -p schist-tools-type
lint-editable-interchange:
	$(CARGO) clippy -p schist-codec-affinity -p schist-codec-psd -p schist-text-engine -p schist-tools-type --all-targets -- -D warnings
fmt-editable-interchange:
	$(CARGO) fmt -p schist-codec-affinity -p schist-codec-psd -p schist-text-engine -p schist-tools-type
inspect-affinity-interchange:
	$(CARGO) run -p schist-codec-affinity --example afschema -- $(AFFINITY_FIXTURE) $(AFFINITY_CLASS)

.PHONY: inspect-affinity-text-runs
inspect-affinity-text-runs:
	$(CARGO) run -p schist-codec-affinity --example aftextruns -- $(AFFINITY_FIXTURE)

.PHONY: check-editable-interchange-web
check-editable-interchange-web:
	$(CARGO) check -p schist-codec-affinity -p schist-codec-psd -p schist-text-engine -p schist-tools-type --target wasm32-unknown-unknown

.PHONY: check-editable-interchange-independent
check-editable-interchange-independent:
	node scripts/check-psd-text-interchange.cjs "$(AG_PSD_MODULE)" "$(SCHIST_INTERCHANGE_ARTIFACT_DIR)"
check-gpu-fx:
	$(CARGO) test -p schist-fx -p schist-filters-core -p schist-compositor-gpu

.PHONY: fmt-gpu-fx lint-gpu-fx check-gpu-shaders
check-gpu-shaders:
	$(CARGO) test -p schist-compositor-gpu --test effect_shaders

.PHONY: check-gpu-opportunities
check-gpu-opportunities:
	$(CARGO) test -p schist-compositor-gpu --test adjustment_coverage --test compute_programs --test async_filters --test native_parity --test remaining_opportunities

.PHONY: check-gpu-domains
check-gpu-domains:
	$(CARGO) test -p schist-adjustments -p schist-core -p schist-layer-fx -p schist-codec-raw -p schist-colormgmt -p schist-vector -p schist-neural -p schist-tools-retouch -p schist-tools-transform -p schist-tools-select -p schist-commands-core -p schist-codecs-common

.PHONY: check-web-gpu
check-web-gpu:
	$(CARGO) check -p schist-compositor-gpu -p schist-editor --target wasm32-unknown-unknown

.PHONY: test-web-gpu fmt-web-gpu lint-web-gpu web-debug
test-web-gpu:
	CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner $(CARGO) test -p schist-compositor-gpu --target wasm32-unknown-unknown --test browser

fmt-web-gpu:
	$(CARGO) fmt -p schist-compositor-gpu -p schist-editor -p schist-fx -p schist-filters-core -p schist-plugin-api -p schist-adjustments -p schist-core -p schist-layer-fx -p schist-codec-raw -p schist-colormgmt -p schist-vector -p schist-neural -p schist-tools-retouch

lint-web-gpu:
	$(CARGO) clippy -p schist-compositor-gpu -p schist-editor --target wasm32-unknown-unknown -- -D warnings

web-debug:
	tools/web-build.sh --debug

.PHONY: bench-paint-web check-web-interaction test-web-drop
test-web-drop:
	CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner $(CARGO) test -p schist-app-platform --target wasm32-unknown-unknown --test browser_drop

bench-paint-web:
	$(CARGO) build --profile web -p schist-tools-paint --example paintbench --target wasm32-unknown-unknown
	wasm-bindgen --target nodejs --out-dir "$${CARGO_TARGET_DIR:-target}/paintbench" "$${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/web/examples/paintbench.wasm"
	node tools/bench-paint.mjs "$${CARGO_TARGET_DIR:-target}/paintbench/paintbench.js"

check-web-interaction:
	$(CARGO) test -p schist-tools-paint -p schist-tools-transform
	$(CARGO) test -p schist-editor --lib workspace::viewport_frame::tests
	$(MAKE) check-app-web

fmt-gpu-fx:
	$(CARGO) fmt -p schist-fx -p schist-filters-core -p schist-compositor-gpu
lint-gpu-fx:
	$(CARGO) clippy -p schist-fx -p schist-filters-core -p schist-compositor-gpu --all-targets -- -D warnings
check-readme: check-text check-gpu-fx
	$(CARGO) clippy -p schist-core -p schist-text-engine -p schist-tools-type -p schist-codec-affinity -p schist-fx -p schist-compositor-gpu --all-targets -- -D warnings
# Native cloud client and editor integration checks.
.PHONY: check-cloud
check-cloud:
	$(CARGO) test -p schist-cloud
	$(CARGO) test -p schist-editor cloud_lifecycle_tests
	$(CARGO) test -p schist-cloud-transfer
	$(CARGO) check -p schist-app

# Requires wasm-bindgen-test-runner and a browser WebDriver (e.g. CHROMEDRIVER).
.PHONY: check-cloud-wasm
check-cloud-wasm:
	$(CARGO) check -p schist-app --target wasm32-unknown-unknown
	CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner $(CARGO) test -p schist-cloud -p schist-document --target wasm32-unknown-unknown --lib

# Shared desktop/cloud document engine; no GPUI or network client.
.PHONY: document-worker check-document format-document
document-worker:
	$(CARGO) build $(PROFILE_FLAG) -p schist-document --bin schist-document-worker
check-document:
	$(CARGO) test -p schist-document
format-document:
	$(CARGO) fmt -p schist-document -p schist-cloud -p schist-codecs-common

# The cloud adapter uses the same face detector, recogniser and crop as desktop.
.PHONY: check-people
check-people:
	$(CARGO) test -p schist-gallery people
	$(CARGO) test -p schist-neural faces
	$(CARGO) check -p schist-library

.PHONY: format-cloud
format-cloud:
	$(CARGO) fmt -p schist-cloud $(APP_PACKAGES) -p schist-document -p schist-library

.PHONY: check-gallery check-cloud-browser
check-gallery:
	$(CARGO) test -p schist-editor -p schist-gallery-ui -p schist-map-view

.PHONY: check-version-history
check-version-history:
	$(CARGO) test -p schist-gallery -p schist-editor --lib versions::tests
	$(CARGO) test -p schist-editor --lib workspace::library_ops::tests::moving_a_photo
	$(CARGO) test -p schist-editor --lib workspace::library_ops::tests::batch_process_

check-cloud-browser:
	$(CARGO) check -p schist-app --target wasm32-unknown-unknown

.PHONY: check-feature-flags
check-feature-flags:
	$(CARGO) test -p schist-app-settings --lib feature_flags::
	$(CARGO) test -p schist-editor --lib feature_flag_tests::
	$(CARGO) test -p schist-app --test feature_flags

.PHONY: lint-cloud
lint-cloud:
	$(CARGO) clippy -p schist-cloud $(APP_PACKAGES) -p schist-document -p schist-library --all-targets -- -D warnings

# Photoshop palette readers, workspace integration, and browser compilation.
.PHONY: check-palettes check-palettes-wasm
check-palettes:
	$(CARGO) test -p schist-color
	$(CARGO) test -p schist-editor --lib workspace::palettes::tests
	$(CARGO) test -p schist-i18n
check-palettes-wasm:
	$(CARGO) check -p schist-app --target wasm32-unknown-unknown

.PHONY: check-mask-refinement
check-mask-refinement:
	$(CARGO) test -p schist-core mask_refine
	$(CARGO) test -p schist-core payload_budget_tests
	$(CARGO) check -p schist-editor --all-targets

# Native video decoding, gallery invariants, and catalogs.
.PHONY: check-video format-video
check-video:
	$(CARGO) test -p schist-gallery -p schist-i18n
	$(CARGO) test -p schist-app -p schist-editor -p schist-video
	$(MAKE) check-video-native

.PHONY: check-video-native
check-video-native:
	$(CARGO) test -p schist-video native_tests -- --include-ignored
format-video:
	$(CARGO) fmt -p schist-app -p schist-editor -p schist-gallery -p schist-video

.PHONY: lint-video
lint-video:
	$(CARGO) clippy -p schist-app -p schist-editor -p schist-gallery -p schist-video --all-targets -- -D warnings

# Regenerate our small native-encoded H.264 test clips (macOS only).
.PHONY: video-fixtures
video-fixtures:
	swift tools/video-fixtures.swift crates/video/tests/fixtures/video

.PHONY: check-video-ios check-video-android
check-video-ios:
	$(CARGO) check -p schist-app --target aarch64-apple-ios
check-video-android:
	./tools/android-build.sh --check

.PHONY: check-video-android-native check-video-ios-native
check-video-android-native:
	./tools/android-build.sh --video-test
check-video-ios-native:
	./tools/ios-test.sh -p schist-video --lib

# Native channel storage, edits, processing boundaries and file round trips.
.PHONY: check-native-color
check-native-color:
	$(CARGO) test -p schist-color -p schist-core -p schist-colormgmt -p schist-codec-psd -p schist-compositor -p schist-tools-paint -p schist-plugin-api -p schist-document -p schist-commands-core

.PHONY: format-native-color lint-native-color check-native-color-app
NATIVE_COLOR_PACKAGES := -p schist-color -p schist-core -p schist-colormgmt -p schist-plugin-api -p schist-codec-psd -p schist-compositor -p schist-compositor-gpu -p schist-document -p schist-codec-affinity -p schist-editor -p schist-tools-paint -p schist-commands-core -p schist-mcp
format-native-color:
	$(CARGO) fmt $(NATIVE_COLOR_PACKAGES)
lint-native-color:
	$(CARGO) clippy $(NATIVE_COLOR_PACKAGES) --all-targets -- -D warnings
check-native-color-app:
	$(CARGO) check -p schist-app --all-targets

# GPU native-channel parity. Set SCHIST_REQUIRE_GPU=1 to reject missing adapters.
.PHONY: check-native-color-gpu
check-native-color-gpu:
	$(CARGO) test -p schist-compositor-gpu --test native_parity -- --nocapture
	$(CARGO) test -p schist-compositor-gpu --test parity

# Headless shared library and WebAssembly bindings; UI embedding is separate.
.PHONY: library library-native library-wasm check-library check-library-wasm format-library
LIBRARY_TARGET_DIR ?= $(or $(SCHIST_LIBRARY_TARGET_DIR),target/library)
library: library-native library-wasm
library-native:
	CARGO='$(CARGO)' SCHIST_LIBRARY_TARGET_DIR='$(LIBRARY_TARGET_DIR)' ./tools/library-cargo.sh build $(PROFILE_FLAG) --lib
	@mkdir -p dist/library
	cp include/schist.h LICENSE web/fonts/LICENSE-IBMPlexSans.txt dist/library/
ifeq ($(HOST),linux)
	cp '$(LIBRARY_TARGET_DIR)/$(PROFILE)/libschist.so' dist/library/
else ifeq ($(HOST),macos)
	cp '$(LIBRARY_TARGET_DIR)/$(PROFILE)/libschist.dylib' dist/library/
else ifeq ($(HOST),windows)
	cp '$(LIBRARY_TARGET_DIR)/$(PROFILE)/schist.dll' dist/library/
endif
library-wasm:
	CARGO='$(CARGO)' SCHIST_LIBRARY_TARGET_DIR='$(LIBRARY_TARGET_DIR)' ./tools/library-build.sh $(if $(filter debug,$(PROFILE)),--debug,)
check-library:
	CARGO='$(CARGO)' ./tools/library-cargo.sh test --lib --tests
check-library-wasm:
	CARGO='$(CARGO)' ./tools/library-cargo.sh check --target wasm32-unknown-unknown
format-library:
	$(CARGO) fmt -p schist-library -p schist-mcp

.PHONY: smoke-library lint-library
smoke-library: library
	$(CC) -Wall -Wextra -Werror -Iinclude examples/library/smoke.c -Ldist/library -lschist -Wl,-rpath,'$(CURDIR)/dist/library' -o '$(LIBRARY_TARGET_DIR)/smoke-c'
	'$(LIBRARY_TARGET_DIR)/smoke-c'
	node examples/library/smoke.cjs
lint-library:
	CARGO='$(CARGO)' ./tools/library-cargo.sh clippy --lib --tests -- -D warnings

# Editable filter recipe rendering, native source preservation and persistence.
.PHONY: check-filter-stacks
check-filter-stacks:
	$(CARGO) test -p schist-core --test filter_stacks
	$(CARGO) test -p schist-plugin-api filter_stack
	$(CARGO) test -p schist-codec-psd --test filter_stacks
	$(CARGO) test -p schist-document filter_stack
	$(CARGO) test -p schist-editor --lib filter_stack

.PHONY: format-filter-stacks
format-filter-stacks:
	$(CARGO) fmt -p schist-core -p schist-plugin-api -p schist-codec-psd -p schist-document -p schist-editor

# Multi-output export recipes: real codecs, naming, persistence and source preservation.
.PHONY: check-export-recipes
check-export-recipes:
	$(CARGO) test -p schist-editor export_recipes

# Live filter placement, UI transforms, source preservation and file round trips.
.PHONY: check-live-stack-transforms check-live-stack-kernels check-live-stack-storage check-live-stack-editor format-live-stack-transforms
check-live-stack-transforms: check-live-stack-kernels check-live-stack-storage check-live-stack-editor
check-live-stack-kernels:
	$(CARGO) test -p schist-core --test filter_stacks
	$(CARGO) test -p schist-tools-transform -p schist-tools-basic -p schist-commands-core
check-live-stack-storage:
	$(CARGO) test -p schist-codec-psd --test filter_stacks
	$(CARGO) test -p schist-document filter_stack
check-live-stack-editor:
	$(CARGO) test -p schist-editor --lib filter_stack
format-live-stack-transforms:
	$(CARGO) fmt -p schist-core -p schist-editor -p schist-tools-transform -p schist-commands-core -p schist-codec-psd -p schist-document -p schist-plugin-api

# Requires a real GPU adapter; exercises async preview/apply and native moves.
.PHONY: check-live-stack-gpu format-live-stack-gpu
check-live-stack-gpu:
	SCHIST_REQUIRE_GPU=1 $(CARGO) test -p schist-compositor-gpu --test remaining_opportunities asynchronous_tool_edits_preserve_preview_and_undo_contracts -- --exact --nocapture
format-live-stack-gpu:
	$(CARGO) fmt -p schist-compositor-gpu

.PHONY: check-smart-objects test-smart-objects test-smart-object-model
check-smart-objects:
	$(CARGO) check -p schist-editor --all-targets
test-smart-object-model:
	$(CARGO) test -p schist-core --test smart_sources
	$(CARGO) test -p schist-codec-psd --test smart_sources
	$(CARGO) test -p schist-codec-psd --test writer smart
	$(CARGO) test -p schist-codec-psd --lib smart::tests
	$(CARGO) test -p schist-document --test smart_sources
test-smart-objects: test-smart-object-model
	$(CARGO) test -p schist-editor smart_objects::tests

.PHONY: check-extended-actions format-extended-actions
check-extended-actions: check-recordable-actions
	$(CARGO) test -p schist-tools-transform action_transform
	$(CARGO) test -p schist-plugin-api filter_stack
format-extended-actions:
	$(CARGO) fmt -p schist-editor -p schist-plugin-api -p schist-tools-transform
# Unicode bidi, vertical shaping, canvas editing and saved text regressions.
.PHONY: check-text-directions format-text-directions
check-text-directions: check-text
	$(CARGO) check -p schist-editor --all-targets
	python3 tools/check-i18n.py
	python3 tools/sync-i18n.py --check
format-text-directions:
	$(CARGO) fmt -p schist-text-engine -p schist-tools-type -p schist-editor -p schist-codec-affinity
.PHONY: check-text-direction-editing lint-text-directions
check-text-direction-editing:
	$(CARGO) test -p schist-text-engine -p schist-tools-type
lint-text-directions:
	$(CARGO) clippy -p schist-text-engine -p schist-tools-type --all-targets -- -D warnings
.PHONY: check-text-direction-editor
check-text-direction-editor:
	$(CARGO) check -p schist-editor --all-targets
# Parameter-backed canvas filter handles and their editor integration.
.PHONY: check-filter-canvas check-filter-canvas-model format-filter-canvas
check-filter-canvas: check-filter-canvas-model
	$(CARGO) test -p schist-editor --lib filter_canvas
check-filter-canvas-model:
	$(CARGO) test -p schist-plugin-api filter_canvas
	$(CARGO) test -p schist-filters-core --test canvas_controls
format-filter-canvas:
	$(CARGO) fmt -p schist-plugin-api -p schist-filters-core -p schist-editor -p schist-ui

.PHONY: format-richer-brushes test-richer-brushes check-richer-brushes-web
format-richer-brushes:
	$(CARGO) fmt -p schist-plugin-api -p schist-app-settings -p schist-tools-paint -p schist-editor -p schist-app-platform
test-richer-brushes:
	$(CARGO) test -p schist-tools-paint -p schist-app-settings -p schist-plugin-api
check-richer-brushes-web:
	$(CARGO) check -p schist-editor --target wasm32-unknown-unknown

.PHONY: lint-richer-brushes
lint-richer-brushes:
	$(CARGO) clippy -p schist-plugin-api -p schist-app-settings -p schist-tools-paint -p schist-editor -p schist-app-platform --all-targets -- -D warnings

.PHONY: check-similar-photos test-similar-photos fmt-similar-photos
check-similar-photos:
	$(CARGO) check -p schist-editor --all-targets
test-similar-photos:
	$(CARGO) test -p schist-gallery similar
fmt-similar-photos:
	$(CARGO) fmt -p schist-gallery -p schist-editor

.PHONY: check-similar-locales
check-similar-locales:
	python3 tools/check-i18n.py --files library.lang
	python3 tools/sync-i18n.py --check

.PHONY: check-symmetry test-symmetry fmt-symmetry
check-symmetry:
	$(CARGO) check -p schist-editor -p schist-tools-paint -p schist-compositor -p schist-plugin-api --all-targets
test-symmetry:
	$(CARGO) test -p schist-tools-paint -p schist-plugin-api -p schist-compositor
	$(CARGO) test -p schist-editor --lib viewport_frame
fmt-symmetry:
	$(CARGO) fmt -p schist-plugin-api -p schist-editor -p schist-tools-paint -p schist-compositor

.PHONY: check-symmetry-i18n
check-symmetry-i18n:
	python3 tools/check-i18n.py
	python3 tools/sync-i18n.py --check

.PHONY: check-symmetry-format
check-symmetry-format:
	$(CARGO) fmt --all -- --check

# Portable XMP metadata and the gallery's batch editor/move integration.
.PHONY: check-metadata-xmp format-metadata-xmp
check-metadata-xmp:
	$(CARGO) test -p schist-gallery xmp
	$(CARGO) test -p schist-editor --lib metadata
	$(CARGO) test -p schist-editor --lib moving_a_photo
	$(CARGO) check -p schist-editor --all-targets
format-metadata-xmp:
	$(CARGO) fmt -p schist-gallery -p schist-editor

.PHONY: check-metadata-catalogs
check-metadata-catalogs:
	python3 tools/check-i18n.py
	python3 tools/sync-i18n.py --check

# Optional independent interoperability oracle (requires ExifTool).
.PHONY: check-metadata-xmp-exiftool
check-metadata-xmp-exiftool:
	$(CARGO) test -p schist-gallery xmp_interoperates_with_exiftool -- --ignored

.PHONY: check-metadata-xmp-core check-metadata-xmp-native lint-metadata-xmp
check-metadata-xmp-core:
	$(CARGO) test -p schist-gallery xmp
check-metadata-xmp-native:
	$(CARGO) check -p schist-editor --all-targets
lint-metadata-xmp:
	$(CARGO) clippy -p schist-gallery -p schist-editor --all-targets -- -D warnings

# Local photo decisions, persistence and the synchronized comparison viewer.
.PHONY: check-photo-culling check-photo-culling-app format-photo-culling lint-photo-culling
check-photo-culling:
	$(CARGO) test -p schist-gallery -p schist-editor -p schist-app-actions --lib culling
	$(CARGO) check -p schist-editor --all-targets
check-photo-culling-app:
	$(CARGO) check -p schist-app --all-targets
format-photo-culling:
	$(CARGO) fmt -p schist-gallery -p schist-editor -p schist-app-actions

lint-photo-culling:
	$(CARGO) clippy -p schist-gallery -p schist-editor -p schist-app-actions --all-targets -- -D warnings

.PHONY: fmt-spot-ink test-spot-ink check-spot-ink
fmt-spot-ink:
	$(CARGO) fmt -p schist-core -p schist-codec-psd -p schist-codecs-common -p schist-document -p schist-tools-paint -p schist-tools-transform -p schist-commands-core -p schist-editor
test-spot-ink:
	$(CARGO) test -p schist-core -p schist-codec-psd -p schist-codecs-common -p schist-document -p schist-tools-paint -p schist-tools-transform -p schist-commands-core
check-spot-ink:
	$(CARGO) check -p schist-editor --all-targets

SPOT_PROBE_DIR ?= /tmp/schist-spot-probe
.PHONY: verify-spot-psd test-spot-editor
verify-spot-psd:
	$(CARGO) run -p schist-codec-psd --example spot_probe -- $(SPOT_PROBE_DIR)
	python3 tools/check-spot-psd.py $(SPOT_PROBE_DIR)
test-spot-editor:
	$(CARGO) test -p schist-editor spot_geometry_tests --lib

.PHONY: fmt-check-spot-ink check-spot-web
fmt-check-spot-ink:
	$(CARGO) fmt --all -- --check
check-spot-web:
	$(CARGO) check -p schist-editor --target wasm32-unknown-unknown

.PHONY: check-spot-catalogs
check-spot-catalogs:
	python3 tools/check-i18n.py
	python3 tools/sync-i18n.py --check

.PHONY: lint-spot-ink
lint-spot-ink:
	$(CARGO) clippy -p schist-core -p schist-codec-psd -p schist-codecs-common -p schist-document -p schist-plugin-api -p schist-tools-paint -p schist-tools-transform -p schist-commands-core -p schist-editor --all-targets -- -D warnings

.PHONY: check-cloud-gallery test-cloud-gallery format-cloud-gallery
check-cloud-gallery:
	$(CARGO) check -p schist-cloud -p schist-editor --all-targets
test-cloud-gallery:
	$(CARGO) test -p schist-cloud
format-cloud-gallery:
	$(CARGO) fmt -p schist-cloud -p schist-editor

.PHONY: check-gallery-ui test-gallery-ui format-gallery-ui lint-gallery-ui
check-gallery-ui:
	$(CARGO) check -p schist-gallery-ui -p schist-editor --all-targets
test-gallery-ui:
	$(CARGO) test -p schist-gallery -p schist-gallery-ui -p schist-editor --lib culling
	$(CARGO) test -p schist-editor --lib metadata
format-gallery-ui:
	$(CARGO) fmt -p schist-gallery -p schist-gallery-ui -p schist-editor
lint-gallery-ui:
	$(CARGO) clippy -p schist-gallery -p schist-gallery-ui -p schist-editor --all-targets -- -D warnings

.PHONY: lint-cloud-gallery
lint-cloud-gallery:
	$(CARGO) clippy -p schist-cloud -p schist-editor --all-targets -- -D warnings

.PHONY: test-cloud-review
test-cloud-review:
	$(CARGO) test -p schist-cloud gallery::tests
	$(CARGO) test -p schist-gallery similar
	$(CARGO) test -p schist-editor --lib review_api_pages

.PHONY: test-cloud-content-filter
test-cloud-content-filter:
	$(CARGO) test -p schist-cloud content_filter
	$(CARGO) test -p schist-gallery scores::tests

.PHONY: test-bucket-content-filter format-bucket-content-filter
test-bucket-content-filter:
	$(CARGO) test -p schist-gallery -p schist-editor -p schist-cloud --lib bucket
format-bucket-content-filter:
	$(CARGO) fmt -p schist-gallery -p schist-editor -p schist-cloud

.PHONY: test-camera-import-progress format-camera-import-progress
test-camera-import-progress:
	$(CARGO) test -p schist-cloud-transfer
	$(CARGO) test -p schist-editor --lib cloud_lifecycle_tests
format-camera-import-progress:
	$(CARGO) fmt -p schist-cloud-transfer -p schist-editor

# Lensfun model parsing, calibration math, and portable filter recipes.
.PHONY: check-lens-profiles test-lens-profiles
check-lens-profiles:
	$(CARGO) check -p schist-filters-core -p schist-editor --all-targets
test-lens-profiles:
	$(CARGO) test -p schist-filters-core lens_profiles
.PHONY: lint-lens-profiles
lint-lens-profiles:
	$(CARGO) clippy -p schist-filters-core -p schist-editor -p schist-gallery --all-targets -- -D warnings
.PHONY: check-lens-profiles-mcp
check-lens-profiles-mcp:
	$(CARGO) check -p schist-mcp --all-targets
.PHONY: test-lens-profiles-mcp
test-lens-profiles-mcp:
	$(CARGO) test -p schist-mcp --no-default-features --lib lens_profiles_mcp

.PHONY: test-printing fmt-printing lint-printing
test-printing:
	$(CARGO) test -p schist-editor -p schist-gallery printing
lint-printing:
	$(CARGO) clippy -p schist-editor -p schist-gallery -p schist-app-actions --all-targets -- -D warnings
fmt-printing:
	$(CARGO) fmt -p schist-editor -p schist-app-actions -p schist-gallery

# Native pen routing and existing brush fallback/rotation regressions.
.PHONY: test-native-stylus-tilt fmt-native-stylus-tilt
test-native-stylus-tilt:
	$(CARGO) test -p schist-plugin-api brush::tests
	$(CARGO) test -p schist-tools-paint tilt
fmt-native-stylus-tilt:
	$(CARGO) fmt -p schist-editor -p schist-ui -p schist-app-platform -p schist-plugin-api -p schist-tools-paint

.PHONY: check-native-stylus-toolkit-windows check-native-stylus-toolkit-macos
check-native-stylus-toolkit-windows:
	$(CARGO) check -p gpui --target x86_64-pc-windows-gnu
check-native-stylus-toolkit-macos:
	$(CARGO) check -p gpui --target aarch64-apple-darwin

.PHONY: test-native-stylus-toolkit
# Cargo cannot run a non-workspace dependency's dev-dependency tests. Supply
# an isolated checkout of the pinned fork; this never modifies Cargo's cache.
test-native-stylus-toolkit:
	@test -n "$(GPUI_CHECKOUT)" || (echo 'Set GPUI_CHECKOUT to an isolated checkout of the pinned GPUI fork'; exit 1)
	$(CARGO) test --manifest-path "$(GPUI_CHECKOUT)/Cargo.toml" --lib tilt_tests

.PHONY: lint-native-stylus-tilt
lint-native-stylus-tilt:
	$(CARGO) clippy -p schist-editor -p schist-ui -p schist-app-platform -p schist-tools-paint --all-targets -- -D warnings

.PHONY: test-tethered check-tethered fmt-tethered
# Override TETHERED_TARGET for a backend-only cross-check, without cross-linking
# the editor's GPU/codec dependencies.
TETHERED_TARGET ?=
.PHONY: check-tethered-backend test-tethered-editor test-tethered-web
.PHONY: lint-tethered-backend
lint-tethered-backend:
	$(CARGO) clippy -p schist-tethered --all-targets $(if $(TETHERED_TARGET),--target $(TETHERED_TARGET),) -- -D warnings
.PHONY: test-tethered-android
test-tethered-android:
	./tools/test-tethered-android.sh
test-tethered-web:
	node --test web/tethered.test.mjs
check-tethered-backend:
	$(CARGO) check -p schist-tethered --all-targets $(if $(TETHERED_TARGET),--target $(TETHERED_TARGET),)
test-tethered-editor:
	$(CARGO) test -p schist-editor library_icc::tests
.PHONY: test-tethered-cloud
test-tethered-cloud:
	$(CARGO) test -p schist-editor tethered_cloud::tests
test-tethered:
	$(CARGO) test -p schist-tethered
.PHONY: test-tethered-webcam-discovery test-tethered-webcam
test-tethered-webcam-discovery:
	$(CARGO) test -p schist-tethered webcam::tests::hardware_discovery -- --ignored --nocapture
# Opt-in: opens the first webcam and may prompt for macOS camera access.
test-tethered-webcam:
	$(CARGO) test -p schist-tethered webcam::tests::hardware_capture_and_cancel -- --ignored --nocapture
check-tethered:
	$(CARGO) check -p schist-tethered -p schist-editor --all-targets
fmt-tethered:
	$(CARGO) fmt -p schist-tethered -p schist-editor -p schist-app-platform
.PHONY: lint-tethered
lint-tethered:
	$(CARGO) clippy -p schist-tethered -p schist-editor --all-targets -- -D warnings
.PHONY: test-virtual-copies check-virtual-copies format-virtual-copies lint-virtual-copies
test-virtual-copies:
	$(CARGO) test -p schist-gallery --lib variants::tests
	$(CARGO) test -p schist-gallery --lib scan::tests
	$(CARGO) test -p schist-gallery --lib versions::tests
	$(CARGO) test -p schist-editor --lib workspace::library_ops::tests::virtual_copy
check-virtual-copies:
	$(CARGO) check -p schist-gallery -p schist-editor --all-targets
format-virtual-copies:
	$(CARGO) fmt -p schist-gallery -p schist-editor
lint-virtual-copies:
	$(CARGO) clippy -p schist-gallery -p schist-editor --all-targets -- -D warnings

.PHONY: test-workspace-presets fmt-workspace-presets
test-workspace-presets:
	$(CARGO) test -p schist-app-settings workspaces::tests
fmt-workspace-presets:
	$(CARGO) fmt -p schist-app-settings -p schist-editor -p schist-app-actions -p schist-app-platform
.PHONY: lint-workspace-presets
lint-workspace-presets:
	$(CARGO) clippy -p schist-app-settings -p schist-editor -p schist-app-actions -p schist-app-platform --all-targets -- -D warnings
