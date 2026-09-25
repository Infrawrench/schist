#!/usr/bin/env python3
"""Validate catalogs independently of which locales are currently built.

Use --locale fr [--files app.lang menu.lang] while translating. With no
locale, checks every registered catalog. --all-iso also checks the entire
ISO inventory and that all of it is registered. Structural checks cannot
certify translation quality: --audit lists unchanged English prose for review;
--strict-audit also fails when that prose remains untranslated.
"""

import argparse
import json
import re
import xml.etree.ElementTree as ET
from functools import cache
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
I18N = ROOT / "crates/i18n"
PLACEHOLDERS = re.compile(r"\{([^{}]+)\}")
SHELL_COMMANDS = re.compile(r"\bsudo [^\\\n]+")
INLINE_CODE = re.compile(r"`([^`]+)`")
# These are user-entered enum values consumed by workspace/cloud.rs::parse_filters.
# Translate their surrounding explanation, but retain the accepted input words.
LITERAL_VALUES = {
    "cloud.error.edited_filter": ("any", "yes", "no"),
    "cloud.error.content_filter": ("all", "safe", "flagged"),
}
PRODUCTS = ("Schist Cloud", "Schist", "Photoshop", "Camera Raw", "Neural Filters", "Lensfun")
# These two labels describe a raw camera file, not the Camera Raw product.
GENERIC_RAW_KEYS = {"common.filetype.raw", "codec.raw.name"}
BREADCRUMBS = {
    "ai.status.panel_hidden": ("menu.view", "menu.view.ai_panel"),
    "library.search_models.download_note": ("menu.gallery", "menu.filter.manage_models"),
    "filter.neural.msg.get_model": ("menu.filter", "filter.category.neural", "menu.filter.manage_models"),
    "filter.neural.style_transfer.msg.no_styles": ("menu.filter", "filter.category.neural", "menu.filter.manage_models"),
}


@cache
def one_requires_number():
    """Languages whose CLDR integer examples include a non-1 `one` count."""
    result = set()
    for group in ET.parse(I18N / "data/plurals.xml").findall(".//pluralRules"):
        for rule in group:
            text = rule.text or ""
            if rule.get("count") != "one" or "@integer" not in text:
                continue
            section = text.split("@integer", 1)[1].split("@decimal", 1)[0]
            bounds = [int(bound) for item in section.split(",")
                      if item.strip() != "…" and "c" not in item
                      for bound in item.strip().split("~") if bound.isdigit()]
            if any(n != 1 for n in bounds):
                result.update(group.get("locales").split())
    return result


def tags(path):
    return [line.split("\t")[0] for line in path.read_text().splitlines()
            if line.strip() and not line.startswith("#")]


def read_catalog(directory, files=None):
    entries, errors = {}, []
    paths = [directory / name for name in files] if files else sorted(directory.glob("*.lang"))
    for path in paths:
        if not path.exists():
            errors.append(f"missing file {path}")
            continue
        for number, line in enumerate(path.read_text().splitlines(), 1):
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            if "=" not in line:
                errors.append(f"{path}:{number}: expected key = value")
                continue
            key, value = [part.strip() for part in line.split("=", 1)]
            if key in entries:
                errors.append(f"{path}:{number}: duplicate {key}")
            if not value:
                errors.append(f"{path}:{number}: empty {key}")
            entries[key] = value
    return entries, errors


def validate(tag, english, files, audit, strict_audit=False):
    categories = json.loads((I18N / "data/plural-categories.json").read_text())
    forms = categories.get(tag.split("-")[0], ["other"])
    stems = {key[:-4] for key in english if key.endswith(".one") and key[:-4] + ".other" in english}
    plural_keys = {stem + suffix for stem in stems for suffix in (".one", ".other")}
    expected = {key: value for key, value in english.items()
                if key not in plural_keys}
    for stem in stems:
        for form in forms:
            source = stem + (".one" if form == "one" else ".other")
            expected[stem + "." + form] = english[source]
    actual, errors = read_catalog(I18N / "locales" / tag, files)
    for name, keys in (("missing", expected.keys() - actual.keys()), ("extra", actual.keys() - expected.keys())):
        if keys:
            errors.append(f"{len(keys)} {name} keys: {', '.join(sorted(keys)[:12])}")
    unchanged = []
    for key in expected.keys() & actual.keys():
        wanted = set(PLACEHOLDERS.findall(expected[key]))
        supplied = set(PLACEHOLDERS.findall(actual[key]))
        # English sometimes leaves the count implicit for exactly one item.
        # A translated `one` form can also cover 21, 101, etc.; tn always
        # supplies n, so those forms may make that count explicit.
        if key.endswith(".one") and key[:-4] in stems and "n" in supplied:
            wanted.add("n")
        if wanted != supplied:
            errors.append(f"{key}: placeholders differ from English")
        if (key.endswith(".one") and key[:-4] in stems
                and tag.split("-")[0] in one_requires_number()
                and "n" not in supplied):
            errors.append(f"{key}: must show {{n}} because 'one' also covers counts other than 1")
        for literal in LITERAL_VALUES.get(key, ()):
            if not re.search(r"(?<![A-Za-z])" + re.escape(literal) + r"(?![A-Za-z])", actual[key]):
                errors.append(f"{key}: preserve the literal input value {literal!r}")
        if key.startswith("app.vulkan."):
            # Diagnostics contain commands users may copy into a shell.
            # Preserve the complete commands, not just package-name substrings.
            commands = lambda text: sorted(command.strip() for command in SHELL_COMMANDS.findall(text))
            if commands(expected[key]) != commands(actual[key]):
                errors.append(f"{key}: shell commands differ from English")
            for literal in INLINE_CODE.findall(expected[key]):
                if literal not in actual[key]:
                    errors.append(f"{key}: preserve the technical literal {literal!r}")
        if "\\`" in actual[key] and "\\`" not in expected[key]:
            errors.append(f"{key}: backticks must not be escaped in .lang files")
        # German compounds can hyphenate a multiword product, e.g.
        # Camera-Raw-Entwicklung. Keep the words without forbidding typography.
        product_text = re.sub(r"[-\u2010-\u2015\s]+", " ", actual[key])
        for product in PRODUCTS:
            if product == "Camera Raw" and key in GENERIC_RAW_KEYS:
                continue
            if product in expected[key] and product not in product_text:
                errors.append(f"{key}: preserve the product name {product!r}")
        if key in BREADCRUMBS:
            # A selected-file audit may not include the menu catalog.
            label_keys = BREADCRUMBS[key]
            if all(label_key in actual for label_key in label_keys):
                labels = [actual[label_key].rstrip("….").rstrip() for label_key in label_keys]
                breadcrumb = " ▸ ".join(labels)
                # Some translations describe navigation in prose, e.g. Chinese
                # "AI Panel in View". Keep that phrasing if it names the labels.
                matches = (breadcrumb in actual[key] if "▸" in actual[key]
                           else all(label in actual[key] for label in labels))
                if not matches:
                    errors.append(f"{key}: breadcrumb must match the menu labels ({breadcrumb!r})")
        # Dimensions such as "{w} × {h} px @ {ppi} ppi · {size}" are
        # language-neutral notation, not untranslated English prose.
        source_words = re.findall(r"[A-Za-z]+", PLACEHOLDERS.sub("", expected[key]))
        if actual[key] == expected[key] and len(source_words) >= 7:
            unchanged.append(key)
    if (audit or strict_audit) and tag != "en" and unchanged:
        message = f"{len(unchanged)} unchanged English sentences: {', '.join(sorted(unchanged))}"
        if strict_audit:
            errors.append(message)
        else:
            print(f"{tag}: review {message}")
    for error in errors:
        print(f"{tag}: {error}")
    if not errors:
        print(f"{tag}: {len(actual)} entries validated")
    return bool(errors)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--locale", action="append", default=[])
    parser.add_argument("--files", nargs="+")
    parser.add_argument("--all-iso", action="store_true")
    parser.add_argument("--audit", action="store_true")
    parser.add_argument("--strict-audit", action="store_true",
                        help="fail on unchanged English prose (seven or more words)")
    args = parser.parse_args()
    registered = tags(I18N / "locales.tsv")
    requested = tags(I18N / "data/iso-639-1.tsv") if args.all_iso else args.locale or registered
    english, errors = read_catalog(I18N / "locales/en", args.files)
    if errors:
        parser.exit(1, "\n".join(errors) + "\n")
    failed = False
    if args.all_iso and set(requested) != set(registered):
        print(f"Registry is missing {len(set(requested) - set(registered))} ISO locales")
        failed = True
    for tag in requested:
        failed |= validate(tag, english, args.files, args.audit, args.strict_audit)
    raise SystemExit(1 if failed else 0)


if __name__ == "__main__":
    main()
