//! The catalogs are data, and these are the checks that keep the data
//! honest: every language has every key, placeholders agree, plurals
//! come in pairs, and nothing in the source tree asks for a key that is
//! not there.

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The active locale is process-wide, so the tests that change it take
/// turns and put English back when they are done.
static LOCALE_LOCK: Mutex<()> = Mutex::new(());

struct EnglishAgain(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl Drop for EnglishAgain {
    fn drop(&mut self) {
        set_locale(Locale::En);
    }
}

fn with_locale(locale: Locale) -> EnglishAgain {
    let guard = LOCALE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_locale(locale);
    EnglishAgain(guard)
}

fn keys(locale: Locale) -> BTreeSet<&'static str> {
    catalogs()[locale.index()].keys().copied().collect()
}

/// Placeholders in a string, as a set: `{n}` and `{name}`.
fn placeholders(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if !after[..close].contains('{') && !after[..close].is_empty() => {
                out.insert(after[..close].to_string());
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

#[test]
fn every_locale_has_a_catalog_directory() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    for locale in Locale::ALL {
        assert!(
            root.join(locale.tag()).is_dir(),
            "no locales/{} directory",
            locale.tag()
        );
    }
    let mut on_disk: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();
    let mut known: Vec<String> = Locale::ALL.iter().map(|l| l.tag().to_string()).collect();
    known.sort();
    assert_eq!(on_disk, known, "a locales/ directory is not a Locale");
}

#[test]
fn english_is_not_empty() {
    assert!(
        keys(Locale::En).len() > 50,
        "the English catalog is missing"
    );
}

#[test]
fn no_key_is_defined_twice() {
    for (locale, source) in Locale::ALL.iter().zip(SOURCES) {
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        for (key, _) in parse(source) {
            *seen.entry(key).or_default() += 1;
        }
        let dupes: Vec<&str> = seen
            .iter()
            .filter(|(_, n)| **n > 1)
            .map(|(k, _)| *k)
            .collect();
        assert!(
            dupes.is_empty(),
            "{} defines these keys more than once: {dupes:?}",
            locale.tag()
        );
    }
}

#[test]
fn keys_are_well_formed() {
    for key in keys(Locale::En) {
        assert!(
            key.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_'),
            "key {key:?} is not lowercase snake case with dots"
        );
        assert!(
            key.contains('.'),
            "key {key:?} has no area prefix (menu., tool., common., …)"
        );
    }
}

#[test]
fn every_translation_has_exactly_the_english_keys() {
    let english = keys(Locale::En);
    for locale in Locale::ALL {
        if locale == Locale::En {
            continue;
        }
        let theirs = keys(locale);
        // A language with no singular is not asked for `.one` strings.
        let wanted: BTreeSet<&str> = english
            .iter()
            .copied()
            .filter(|k| locale.has_singular() || !k.ends_with(".one"))
            .collect();
        let missing: Vec<&&str> = wanted.difference(&theirs).collect();
        let extra: Vec<&&str> = theirs.difference(&wanted).collect();
        assert!(
            missing.is_empty(),
            "{} is missing {} keys, e.g. {:?}",
            locale.tag(),
            missing.len(),
            &missing[..missing.len().min(20)]
        );
        assert!(
            extra.is_empty(),
            "{} has keys English does not: {:?}",
            locale.tag(),
            &extra[..extra.len().min(20)]
        );
    }
}

#[test]
fn translations_keep_the_placeholders() {
    let english = &catalogs()[Locale::En.index()];
    for locale in Locale::ALL {
        for (key, value) in &catalogs()[locale.index()] {
            let Some(source) = english.get(key) else {
                continue;
            };
            assert_eq!(
                placeholders(value),
                placeholders(source),
                "{}: {key} has different placeholders from English",
                locale.tag()
            );
        }
    }
}

#[test]
fn plural_strings_come_in_pairs() {
    for locale in Locale::ALL {
        let theirs = keys(locale);
        for key in &theirs {
            if let Some(stem) = key.strip_suffix(".one") {
                assert!(
                    theirs.contains(format!("{stem}.other").as_str()),
                    "{}: {key} has no .other",
                    locale.tag()
                );
            }
            // `.other` is a plural only when English has the `.one` beside
            // it: `filter.category.other` is a heading, not a count.
            let english = keys(Locale::En);
            if let Some(stem) = key
                .strip_suffix(".other")
                .filter(|stem| english.contains(format!("{stem}.one").as_str()))
            {
                if locale.has_singular() {
                    assert!(
                        theirs.contains(format!("{stem}.one").as_str()),
                        "{}: {key} has no .one",
                        locale.tag()
                    );
                }
                assert!(
                    catalogs()[locale.index()][key].contains("{n}"),
                    "{}: {key} does not show the count",
                    locale.tag()
                );
            }
        }
    }
}

#[test]
fn no_value_is_empty() {
    for locale in Locale::ALL {
        for (key, value) in &catalogs()[locale.index()] {
            assert!(!value.is_empty(), "{}: {key} is empty", locale.tag());
        }
    }
}

/// Every key the source tree asks for exists. A `t("…")` with a typo in
/// it would otherwise show the key on screen, in every language.
#[test]
fn the_source_tree_asks_only_for_keys_that_exist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in ["crates", "plugins"] {
        collect_rust_files(&root.join(dir), &mut files);
    }
    // This crate's own sources spell the calls out in documentation and
    // in these tests, with keys that are examples rather than strings.
    files.retain(|f| !f.starts_with(root.join("crates/i18n")));
    assert!(
        !files.is_empty(),
        "no source files found under {}",
        root.display()
    );
    let english = keys(Locale::En);
    let mut unknown: BTreeSet<String> = BTreeSet::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for key in referenced_keys(&text) {
            let known =
                english.contains(key.as_str()) || english.contains(format!("{key}.other").as_str());
            if !known {
                unknown.insert(format!(
                    "{key} ({})",
                    file.strip_prefix(&root).unwrap().display()
                ));
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "keys asked for but defined in no catalog:\n  {}",
        unknown.iter().cloned().collect::<Vec<_>>().join("\n  ")
    );
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name != "target" {
                collect_rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The string literals that follow `t(`, `tf(`, `tf!(`, `tn(`, `tn!(`,
/// `tnf(` and the lists inside `choices(&[`.
fn referenced_keys(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for call in ["t(\"", "tf(\"", "tf!(\"", "tn(\"", "tn!(\"", "tnf(\""] {
        let mut from = 0;
        while let Some(at) = text[from..].find(call) {
            let start = from + at;
            let preceded_by_ident = start > 0
                && text[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let literal = &text[start + call.len()..];
            if !preceded_by_ident {
                if let Some(end) = literal.find('"') {
                    out.push(literal[..end].to_string());
                }
            }
            from = start + call.len();
        }
    }
    let mut from = 0;
    while let Some(at) = text[from..].find("choices(&[") {
        let start = from + at + "choices(&[".len();
        let Some(end) = text[start..].find(']') else {
            break;
        };
        let list = &text[start..start + end];
        let mut rest = list;
        while let Some(open) = rest.find('"') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('"') else {
                break;
            };
            out.push(after[..close].to_string());
            rest = &after[close + 1..];
        }
        from = start + end;
    }
    out
}

#[test]
fn language_tags_negotiate_by_language_alone() {
    assert_eq!(Locale::from_tag("sv-SE"), Some(Locale::Sv));
    assert_eq!(Locale::from_tag("sv_FI.UTF-8"), Some(Locale::Sv));
    assert_eq!(Locale::from_tag("de-AT"), Some(Locale::De));
    assert_eq!(Locale::from_tag("de_DE@euro"), Some(Locale::De));
    assert_eq!(Locale::from_tag("zh-Hans-CN"), Some(Locale::ZhHans));
    assert_eq!(Locale::from_tag("zh-TW"), Some(Locale::ZhHans));
    assert_eq!(Locale::from_tag("ZH"), Some(Locale::ZhHans));
    assert_eq!(Locale::from_tag("en-GB"), Some(Locale::En));
    assert_eq!(Locale::from_tag("fr-FR"), None);
    assert_eq!(Locale::from_tag("C"), None);
    assert_eq!(Locale::from_tag("POSIX"), None);
    assert_eq!(Locale::from_tag(""), None);
}

#[test]
fn the_first_language_we_have_wins() {
    assert_eq!(Locale::negotiate(["fr-FR", "sv-SE", "en"]), Locale::Sv);
    assert_eq!(Locale::negotiate(["fr-FR", "ja"]), Locale::En);
    assert_eq!(Locale::negotiate(Vec::<String>::new()), Locale::En);
    assert_eq!(Locale::negotiate(["de", "sv"]), Locale::De);
}

#[test]
fn placeholders_are_filled() {
    assert_eq!(fill("Save {name}?", &[("name", &"a.psd")]), "Save a.psd?");
    assert_eq!(fill("{a} and {b}", &[("a", &1), ("b", &2)]), "1 and 2");
    // Unknown placeholders and stray braces stay visible.
    assert_eq!(fill("{x} {", &[]), "{x} {");
    assert_eq!(fill("no braces", &[("x", &1)]), "no braces");
}

#[test]
fn escapes_are_the_two_documented_ones() {
    assert_eq!(unescape("a\\nb"), "a\nb");
    assert_eq!(unescape("a\\\\b"), "a\\b");
    assert_eq!(unescape("a\\qb"), "a\\qb");
    assert_eq!(unescape("trailing\\"), "trailing\\");
}

#[test]
fn parsing_skips_comments_and_blanks() {
    let pairs: Vec<_> = parse("# c\n\n a.b = one two \nc.d=x = y\nbad line\n").collect();
    assert_eq!(pairs, vec![("a.b", "one two"), ("c.d", "x = y")]);
}

#[test]
fn lookups_fall_back_to_english_then_the_key() {
    let _guard = with_locale(Locale::Sv);
    assert_eq!(t("common.cancel"), "Avbryt");
    assert_eq!(t("common.no_such_key_xyz"), "common.no_such_key_xyz");
    set_locale(Locale::En);
    assert_eq!(t("common.cancel"), "Cancel");
}

#[test]
fn counts_pick_the_singular_where_the_language_has_one() {
    let _guard = with_locale(Locale::En);
    assert_eq!(tn("common.n_layers", 1), "1 layer");
    assert_eq!(tn("common.n_layers", 2), "2 layers");
    set_locale(Locale::ZhHans);
    assert_eq!(tn("common.n_layers", 1), "1 个图层");
    set_locale(Locale::De);
    assert_eq!(tn("common.n_layers", 1), "1 Ebene");
    assert_eq!(tn("common.n_layers", 5), "5 Ebenen");
}

#[test]
fn choice_lists_are_translated_and_memoised() {
    let _guard = with_locale(Locale::De);
    static KEYS: &[&str] = &["common.yes", "common.no"];
    let first = choices(KEYS);
    assert_eq!(first, &["Ja", "Nein"]);
    assert!(std::ptr::eq(first, choices(KEYS)));
    set_locale(Locale::En);
    assert_eq!(choices(KEYS), &["Yes", "No"]);
}

#[test]
fn the_macros_name_their_arguments() {
    let _guard = with_locale(Locale::En);
    assert_eq!(
        tf!("common.saved_as", name = "moss.psd"),
        "Saved as moss.psd"
    );
    assert_eq!(tn!("common.n_layers", 3), "3 layers");
}

/// The browser build draws Chinese with a subset of Noto Sans CJK SC
/// (`tools/web-cjk-font.sh`), and a character the subset lacks draws
/// as a box. So: every character the Chinese chrome uses is in it.
#[test]
fn the_web_font_covers_the_chinese_catalog() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/fonts/NotoSansSC-Schist.otf");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|err| panic!("{}: {err}; run tools/web-cjk-font.sh", path.display()));
    let face = ttf_parser::Face::parse(&bytes, 0).expect("a parseable font");
    // Only the CJK characters: Latin, arrows and the like come from the
    // Latin face, for Chinese readers as for everyone else.
    let cjk = |c: char| matches!(c as u32, 0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF);
    let mut missing: BTreeSet<char> = BTreeSet::new();
    for value in catalogs()[Locale::ZhHans.index()].values() {
        for c in value.chars() {
            if cjk(c) && face.glyph_index(c).is_none() {
                missing.insert(c);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "the Chinese web font lacks {}; re-run tools/web-cjk-font.sh",
        missing.iter().collect::<String>()
    );
}
