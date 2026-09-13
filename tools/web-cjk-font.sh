#!/usr/bin/env bash
# The CJK UI fonts for the browser build.
#
# A browser exposes no system fonts to a WebGPU canvas, so the web build
# ships what it draws with (web/fonts/). IBM Plex Sans covers Latin; the
# Chinese and Japanese chrome (crates/i18n/locales/zh-Hans, .../ja) needs
# a CJK face, and a full Noto Sans CJK is 16 MB — the size of the app
# again, twice over. This cuts each one down to what its chrome can
# actually show:
#
#   * every character that language's catalogs use, so nothing in the UI
#     is ever a box;
#   * the common hanzi or kanji, so layer names and file names typed in
#     the language draw too — GB 2312 level 1 (3,755) for Chinese, JIS X
#     0208 level 1 (2,965) plus kana for Japanese;
#   * ASCII, Latin-1, general and CJK punctuation, arrows, geometric
#     shapes, and the fullwidth forms.
#
# About 1.5 MB each, and the loading page fetches only the one the
# reader's browser language asks for (the manifest tags them, and
# loader.js filters on it), so nobody downloads both.
#
# Simplified Chinese and Japanese are separate faces on purpose: the two
# draw many shared characters differently (直, 骨, 令, 海 …), and a
# Japanese reader shown the Chinese forms sees a subtly wrong typeface
# the whole way through.
#
# Re-run when a catalog gains a character its subset lacks:
# `cargo test -p schist-i18n` checks the coverage and says so. Needs
# fonttools (`pip install fonttools`); the source faces are downloaded
# once into target/fonts/.
#
# Noto Sans CJK is under the SIL Open Font License 1.1, which permits
# subsetting; the licence ships beside the fonts as it must.

set -euo pipefail
cd "$(dirname "$0")/.."

BASE_URL="https://github.com/notofonts/noto-cjk/raw/main/Sans"
LICENSE_URL="$BASE_URL/LICENSE"
CACHE=target/fonts

command -v pyftsubset >/dev/null || {
  echo "pyftsubset not found: pip install fonttools" >&2
  exit 1
}

mkdir -p "$CACHE"
[ -s web/fonts/LICENSE-NotoSansCJK.txt ] ||
  curl -sSL --fail -o web/fonts/LICENSE-NotoSansCJK.txt "$LICENSE_URL"

# locale, the source face, the name we ship it under.
build() {
  local locale="$1" face="$2" out="web/fonts/$3"
  local source="$CACHE/$face.otf"
  local text="$CACHE/$locale-chars.txt"

  if [ ! -s "$source" ]; then
    echo "-- fetching $face"
    curl -sSL --fail -o "$source" "$BASE_URL/OTF/$4/$face.otf"
  fi

  echo "-- collecting characters for $locale"
  python3 - "$locale" "$text" <<'EOF'
import glob, sys
locale, out = sys.argv[1], sys.argv[2]
chars = set()
if locale == "zh-Hans":
    # GB 2312 level 1: rows 16-55 of the table, the common hanzi.
    for hi in range(0xB0, 0xD8):
        for lo in range(0xA1, 0xFF):
            try:
                chars.add(bytes([hi, lo]).decode("gb2312"))
            except UnicodeDecodeError:
                pass
else:
    # JIS X 0208 level 1: rows 16-47, the common kanji. The kana and the
    # punctuation in rows 1-15 come from the Unicode ranges below.
    for ku in range(16, 48):
        for ten in range(1, 95):
            try:
                chars.add(bytes([0xA0 + ku, 0xA0 + ten]).decode("euc_jp"))
            except UnicodeDecodeError:
                pass
# Everything this language's chrome says.
for path in glob.glob(f"crates/i18n/locales/{locale}/*.lang"):
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            if line.startswith("#"):
                continue
            chars.update(line.split("=", 1)[-1].strip())
with open(out, "w", encoding="utf-8") as fh:
    fh.write("".join(sorted(c for c in chars if not c.isspace())))
print(f"   {len(chars)} characters")
EOF

  # Hiragana, katakana and the katakana phonetic extensions ride along
  # for Japanese only; they are a few hundred glyphs and the language is
  # unreadable without them.
  local ranges="U+0020-007E,U+00A0-00FF,U+2010-2027,U+2030-205E,U+2190-21FF,U+25A0-25FF,U+3000-303F,U+FF00-FFEF"
  [ "$locale" = ja ] && ranges="$ranges,U+3040-309F,U+30A0-30FF,U+31F0-31FF"

  echo "-- subsetting $locale"
  pyftsubset "$source" \
    --text-file="$text" \
    --unicodes="$ranges" \
    --layout-features='*' \
    --no-hinting \
    --output-file="$out"
  echo "-- $(du -h "$out" | cut -f1) in $out"
}

build zh-Hans NotoSansCJKsc-Regular NotoSansSC-Schist.otf SimplifiedChinese
build ja NotoSansCJKjp-Regular NotoSansJP-Schist.otf Japanese
