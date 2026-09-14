#!/usr/bin/env python3
"""Fetch script fonts for the ISO language inventory and write font locale tags.

The non-CJK Noto faces are small enough to keep complete: this preserves
shaping tables and covers document/layer names as well as translated UI.
Chinese and Japanese retain their existing regional subsets. Korean uses
its own regional face. Every browser fetches only its selected locale's face.
Requires fonttools (also used by web-cjk-font.sh).
"""

import json
import shutil
import urllib.request
from pathlib import Path

from fontTools.ttLib import TTFont
from fontTools import subset

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / "target/fonts"
DEST = ROOT / "web/fonts"
NOTO_REV = "92d2ea744479d3379927f1fb5f8b0945dd0349b7"
CJK_REV = "f8d157532fbfaeda587e826d4cd5b21a49186f7c"
NOTO = f"https://raw.githubusercontent.com/notofonts/notofonts.github.io/{NOTO_REV}/fonts"

# ISO 15924 script -> source family. Shared Latin/Cyrillic/Greek glyphs
# remain in the same face; Han forms use the reader's regional CJK face.
FAMILIES = {
    "Arab": "NotoSansArabic", "Armn": "NotoSansArmenian",
    "Avst": "NotoSansAvestan", "Beng": "NotoSansBengali",
    "Cans": "NotoSansCanadianAboriginal", "Cyrl": "NotoSans",
    "Deva": "NotoSansDevanagari", "Ethi": "NotoSansEthiopic",
    "Geor": "NotoSansGeorgian", "Grek": "NotoSans",
    "Gujr": "NotoSansGujarati", "Guru": "NotoSansGurmukhi",
    "Hebr": "NotoSansHebrew", "Khmr": "NotoSansKhmer",
    "Knda": "NotoSansKannada", "Laoo": "NotoSansLao",
    "Latn": "NotoSans", "Mlym": "NotoSansMalayalam",
    "Mymr": "NotoSansMyanmar", "Orya": "NotoSansOriya",
    "Sinh": "NotoSansSinhala", "Taml": "NotoSansTamil",
    "Telu": "NotoSansTelugu", "Thaa": "NotoSansThaana",
    "Thai": "NotoSansThai", "Tibt": "NotoSerifTibetan",
    "Yiii": "NotoSansYi",
}


def download(url, path):
    if path.exists() and path.stat().st_size:
        return
    temporary = path.with_suffix(path.suffix + ".download")
    try:
        with urllib.request.urlopen(url, timeout=60) as response, temporary.open("wb") as output:
            shutil.copyfileobj(response, output)
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def korean_subset(source, destination):
    # All modern Hangul syllables, jamo, the KS X 1001 repertoire (including
    # common Hanja), and every character in the Korean UI. This keeps Korean
    # layer/file names drawable without shipping the entire pan-CJK font.
    characters = set(range(0x20, 0x100))
    for start, stop in ((0x1100, 0x1200), (0x3130, 0x3190), (0xAC00, 0xD7A4),
                        (0x2010, 0x2060), (0x2190, 0x2200), (0x25A0, 0x2600),
                        (0x3000, 0x3040), (0xFF00, 0xFFF0)):
        characters.update(range(start, stop))
    for high in range(0xA1, 0xFF):
        for low in range(0xA1, 0xFF):
            try:
                characters.update(map(ord, bytes((high, low)).decode("euc_kr")))
            except UnicodeDecodeError:
                pass
    for path in (ROOT / "crates/i18n/locales/ko").glob("*.lang"):
        for line in path.read_text().splitlines():
            if "=" in line and not line.lstrip().startswith("#"):
                characters.update(map(ord, line.split("=", 1)[1]))
    font = TTFont(source)
    options = subset.Options()
    options.layout_features = ["*"]
    options.hinting = False
    cutter = subset.Subsetter(options=options)
    cutter.populate(unicodes=characters)
    cutter.subset(font)
    font.save(destination)


def main():
    CACHE.mkdir(parents=True, exist_ok=True)
    mapping = {"NotoSansSC-Schist.otf": ["zh-Hans"], "NotoSansJP-Schist.otf": ["ja"]}
    for line in (ROOT / "crates/i18n/data/iso-639-1.tsv").read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        tag, _, _, _, script = line.split("\t")
        if script in ("Hans", "Jpan"):
            continue
        if script == "Kore":
            # CJK glyph forms differ by language, so Korean gets its own face.
            source = CACHE / "NotoSansCJKkr-Regular.otf"
            url = f"https://raw.githubusercontent.com/notofonts/noto-cjk/{CJK_REV}/Sans/OTF/Korean/" + source.name
            download(url, source)
            filename = "NotoSansKR-Schist.otf"
            korean_subset(source, DEST / filename)
            mapping[filename] = [tag]
            print(f"{filename}: {(DEST / filename).stat().st_size:,} bytes", flush=True)
            continue
        else:
            family = FAMILIES[script]
            filename = family + "-Regular.ttf"
            url = f"{NOTO}/{family}/hinted/ttf/{filename}"
        mapping.setdefault(filename, []).append(tag)
        source = CACHE / filename
        download(url, source)
        if not (DEST / filename).exists():
            shutil.copyfile(source, DEST / filename)
            print(f"{filename}: {source.stat().st_size:,} bytes", flush=True)
    # Armenian uses U+2024 punctuation, which the Armenian face and IBM
    # Plex lack. The general Noto face supplies it alongside the script face.
    mapping["NotoSans-Regular.ttf"].append("hy")
    # Shortcut glyphs and arrows appear in every language's UI.
    filename = "NotoSansSymbols2-Regular.ttf"
    source = CACHE / filename
    download(f"{NOTO}/NotoSansSymbols2/hinted/ttf/{filename}", source)
    if not (DEST / filename).exists():
        shutil.copyfile(source, DEST / filename)
    mapping[filename] = sorted({tag for locales in mapping.values() for tag in locales})
    # Preserve each font's copyright and licensing information alongside
    # the full OFL text already distributed with the CJK faces.
    credits = ["Noto script fonts", "=================", "", "These unmodified fonts use the SIL Open Font License 1.1.",
               "The full license is in LICENSE-NotoSansCJK.txt.", ""]
    for filename in sorted(mapping):
        if filename.endswith("-Schist.otf"):
            continue
        face = TTFont(DEST / filename)
        credits.append(filename)
        notices = sorted({record.toUnicode() for record in face["name"].names if record.nameID in (0, 13, 14)})
        credits.extend(notices)
        credits.append("")
    (DEST / "LICENSE-NotoScriptFonts.txt").write_text("\n".join(credits))
    (DEST / "locales.json").write_text(json.dumps(mapping, ensure_ascii=False, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
