#!/usr/bin/env bash
# Build an AppImage. Requires `appimagetool` on PATH (or set APPIMAGETOOL).
# The native packages -- .deb, .rpm, .pkg.tar.zst -- are packages.sh's job;
# both ship the same tree, staged by payload.sh.
set -euo pipefail

# shellcheck source=packaging/linux/payload.sh
source "$(dirname "$0")/payload.sh"
root="$payload_root"
# Scratch, not a deliverable: dist/ is uploaded wholesale by CI, and an
# AppDir in there collides with the real artifacts on the release.
appdir="$root/target/Schist.AppDir"
tool="${APPIMAGETOOL:-appimagetool}"

cargo build --release -p schist-app

rm -rf "$appdir"
stage_payload "$appdir"

copy_elf_closure() {
    local queue=("$1") seen="" current source soname
    while ((${#queue[@]})); do
        current="${queue[0]}"
        queue=("${queue[@]:1}")
        [[ -f "$current" ]] || continue
        while read -r first second third _; do
            if [[ "$second" == "=>" ]]; then
                source="$third"
            elif [[ "$first" == /* ]]; then
                source="$first"
            else
                continue
            fi
            [[ "$source" == /* && -f "$source" ]] || continue
            soname="$(basename "$source")"
            case "$soname" in
                libc.so.6|libm.so.6|libdl.so.2|libpthread.so.0|librt.so.1|ld-linux-*) continue ;;
            esac
            [[ "$seen" == *"|$soname|"* ]] && continue
            seen+="|$soname|"
            install -Dm755 "$source" "$appdir/usr/lib/$soname"
            queue+=("$source")
        done < <(ldd "$current" 2>/dev/null || true)
    done
}

mkdir -p "$appdir/usr/lib"
copy_elf_closure "$appdir/usr/bin/schist"
# Camera and I/O drivers live in separate versioned trees on upstream and
# distro installs. Preserve each layout and point libgphoto2 at the directory
# containing the actual modules, not a guessed camlibs/iolibs subdirectory.
bundle_gphoto_modules() {
    local package="$1" tree="$2" source version directory
    source="$(pkg-config --variable=libdir "$package")/$tree"
    [[ -d "$source" ]] || { echo "Missing camera modules: $source" >&2; exit 1; }
    cp -a "$source" "$appdir/usr/lib/$tree"
    directory=""
    for version in "$appdir/usr/lib/$tree"/*; do
        [[ -d "$version" ]] || continue
        if compgen -G "$version/*.so" >/dev/null; then directory="$version"; fi
    done
    [[ -n "$directory" ]] || { echo "No modules in $source" >&2; exit 1; }
    while IFS= read -r -d '' module; do
        copy_elf_closure "$module"
    done < <(find "$appdir/usr/lib/$tree" -type f -name '*.so' -print0)
    basename "$directory"
}
cam_version="$(bundle_gphoto_modules libgphoto2 libgphoto2)"
io_version="$(bundle_gphoto_modules libgphoto2_port libgphoto2_port)"
for copyright in /usr/share/doc/libgphoto2-6*/copyright \
                 /usr/share/doc/libgphoto2-port12*/copyright; do
    [[ -f "$copyright" ]] || continue
    install -Dm644 "$copyright" \
        "$appdir/usr/share/licenses/libgphoto2/$(basename "$(dirname "$copyright")").txt"
done

# appimagetool reads the desktop entry from the AppDir root and looks the
# icon up there by its Icon= key, so both are duplicated out of usr/.
cp "$appdir/usr/share/applications/schist.desktop" "$appdir/schist.desktop"
cp "$appdir/usr/share/icons/hicolor/256x256/apps/com.infrawrench.schist.png" \
   "$appdir/com.infrawrench.schist.png"

cat > "$appdir/AppRun" <<'RUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
export LD_LIBRARY_PATH="$HERE/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export CAMLIBS="$HERE/usr/lib/libgphoto2/__CAM_VERSION__"
export IOLIBS="$HERE/usr/lib/libgphoto2_port/__IO_VERSION__"
exec "$HERE/usr/bin/schist" "$@"
RUN
sed -i -e "s/__CAM_VERSION__/$cam_version/g" -e "s/__IO_VERSION__/$io_version/g" "$appdir/AppRun"
chmod +x "$appdir/AppRun"

out="$root/dist/Schist-$(uname -m).AppImage"
if command -v "$tool" >/dev/null 2>&1; then
    "$tool" "$appdir" "$out"
    echo "built $out"
else
    echo "appimagetool not found; the AppDir is ready at $appdir" >&2
fi
