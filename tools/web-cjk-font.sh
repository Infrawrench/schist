#!/usr/bin/env bash
# The Chinese UI font for the browser build.
#
# A browser exposes no system fonts to a WebGPU canvas, so the web build
# ships what it draws with (web/fonts/). IBM Plex Sans covers Latin;
# for the Chinese chrome (crates/i18n/locales/zh-Hans) it needs a CJK
# face, and the full Noto Sans CJK SC is 16 MB — nearly the size of the
# app. This cuts it down to what the chrome can actually show:
#
#   * every character the zh-Hans catalogs use, so nothing in the UI
#     is ever a box;
#   * GB 2312 level 1, the 3,755 most common hanzi, so layer names and
#     file names typed in Chinese draw too;
#   * ASCII, Latin-1, general and CJK punctuation, and the fullwidth
#     forms.
#
# About 1.5 MB, and the loading page fetches it only when the browser's
# language is Chinese (loader.js reads the `lang` the manifest gives it).
#
# Re-run when the zh-Hans catalogs gain a character the subset lacks:
# `cargo test -p schist-i18n` checks the coverage and says so. Needs
# fonttools (`pip install fonttools`); the source face is downloaded
# once into target/fonts/.
#
# Noto Sans CJK is under the SIL Open Font License 1.1, which permits
# subsetting; the licence ships beside the font as it must.

set -euo pipefail
cd "$(dirname "$0")/.."

SOURCE_URL="https://github.com/notofonts/noto-cjk/raw/main/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf"
LICENSE_URL="https://github.com/notofonts/noto-cjk/raw/main/Sans/LICENSE"
CACHE=target/fonts
SOURCE="$CACHE/NotoSansCJKsc-Regular.otf"
OUT=web/fonts/NotoSansSC-Schist.otf

command -v pyftsubset >/dev/null || {
  echo "pyftsubset not found: pip install fonttools" >&2
  exit 1
}

mkdir -p "$CACHE"
if [ ! -s "$SOURCE" ]; then
  echo "-- fetching Noto Sans CJK SC"
  curl -sSL --fail -o "$SOURCE" "$SOURCE_URL"
  curl -sSL --fail -o web/fonts/LICENSE-NotoSansCJK.txt "$LICENSE_URL"
fi

echo "-- collecting characters"
TEXT="$CACHE/zh-Hans-chars.txt"
python3 - "$TEXT" <<'EOF'
import glob, sys
chars = set()
# GB 2312 level 1: rows 16-55 of the table, the common hanzi.
for hi in range(0xB0, 0xD8):
    for lo in range(0xA1, 0xFF):
        try:
            chars.add(bytes([hi, lo]).decode("gb2312"))
        except UnicodeDecodeError:
            pass
# Everything the Chinese chrome says.
for path in glob.glob("crates/i18n/locales/zh-Hans/*.lang"):
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            if line.startswith("#"):
                continue
            chars.update(line.split("=", 1)[-1].strip())
with open(sys.argv[1], "w", encoding="utf-8") as fh:
    fh.write("".join(sorted(c for c in chars if not c.isspace())))
print(f"   {len(chars)} characters")
EOF

echo "-- subsetting"
pyftsubset "$SOURCE" \
  --text-file="$TEXT" \
  --unicodes="U+0020-007E,U+00A0-00FF,U+2010-2027,U+2030-205E,U+2190-21FF,U+25A0-25FF,U+3000-303F,U+FF00-FFEF" \
  --layout-features='*' \
  --no-hinting \
  --output-file="$OUT"
echo "-- $(du -h "$OUT" | cut -f1) in $OUT"
