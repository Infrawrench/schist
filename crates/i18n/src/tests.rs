//! The catalogs are data, and these are the checks that keep the data
//! honest: every language has every key, placeholders agree, plurals
//! include the required categories, and nothing in the source tree asks for a key that is
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
    catalog(locale).keys().copied().collect()
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
fn compressed_catalogs_match_the_source_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    for locale in Locale::ALL {
        let mut files: Vec<_> = std::fs::read_dir(root.join(locale.tag()))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "lang"))
            .collect();
        files.sort();
        let mut expected = String::new();
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            expected.push_str(&format!(
                "# ==== {} ====\n",
                file.file_name().unwrap().to_str().unwrap()
            ));
            expected.push_str(&text);
            if !text.ends_with('\n') {
                expected.push('\n');
            }
        }
        assert_eq!(source(locale), expected, "{} catalog changed", locale.tag());
    }
}

#[test]
fn shared_dictionary_reduces_the_total_embedded_size() {
    use std::io::Write;

    assert!(!DICTIONARY.is_empty());
    assert!(DICTIONARY.len() <= 32 * 1024);
    let mut without_dictionary = 0;
    for locale in Locale::ALL {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(source(locale).as_bytes()).unwrap();
        without_dictionary += encoder.finish().unwrap().len();
    }
    let compressed = DICTIONARY.len() + SOURCES.iter().map(|(bytes, _)| bytes.len()).sum::<usize>();
    let uncompressed = SOURCES.iter().map(|(_, len)| len).sum::<usize>();
    assert!(
        compressed < without_dictionary,
        "the dictionary must pay for its own storage"
    );
    assert!(compressed < uncompressed / 2);
    println!("catalog bytes: {uncompressed} raw, {without_dictionary} zlib, {compressed} zlib with dictionary");
}

#[test]
fn compressed_catalogs_reject_damage_and_the_wrong_dictionary() {
    let (compressed, len) = SOURCES[Locale::En.index()];
    assert!(decompress(compressed, len, b"wrong dictionary").is_err());
    assert!(decompress(compressed, len - 1, DICTIONARY).is_err());
    assert!(decompress(compressed, len + 1, DICTIONARY).is_err());
    for end in [0, 1, 6, compressed.len() / 2, compressed.len() - 1] {
        assert!(decompress(&compressed[..end], len, DICTIONARY).is_err());
    }
    let mut damaged = compressed.to_vec();
    *damaged.last_mut().unwrap() ^= 1;
    assert!(decompress(&damaged, len, DICTIONARY).is_err());
    let mut trailing = compressed.to_vec();
    trailing.push(0);
    assert!(decompress(&trailing, len, DICTIONARY).is_err());
}

#[test]
fn concurrent_lookups_share_the_decompressed_catalog() {
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8).map(|_| scope.spawn(|| source(Locale::En))).collect();
        for handle in handles {
            assert!(std::ptr::eq(handle.join().unwrap(), source(Locale::En)));
        }
    });
}

#[test]
fn no_key_is_defined_twice() {
    for locale in Locale::ALL {
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        for (key, _) in parse(source(locale)) {
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
        let stems = plural_stems();
        let mut wanted: BTreeSet<String> = english
            .iter()
            .filter(|key| {
                !stems
                    .iter()
                    .any(|stem| **key == format!("{stem}.one") || **key == format!("{stem}.other"))
            })
            .map(|key| key.to_string())
            .collect();
        for stem in stems {
            for category in locale.plural_categories() {
                wanted.insert(format!("{stem}.{category}"));
            }
        }
        let theirs: BTreeSet<String> = theirs.iter().map(|key| key.to_string()).collect();
        let missing: Vec<_> = wanted.difference(&theirs).collect();
        let extra: Vec<_> = theirs.difference(&wanted).collect();
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
    let english = catalog(Locale::En);
    for locale in Locale::ALL {
        for (key, value) in catalog(locale) {
            let alternate = key.rsplit_once('.').and_then(|(stem, category)| {
                (locale.plural_categories().contains(&category)
                    && english.contains_key(format!("{stem}.one").as_str()))
                .then(|| format!("{stem}.other"))
            });
            let source = english
                .get(key)
                .or_else(|| alternate.as_ref().and_then(|key| english.get(key.as_str())))
                .expect("every translated key has an English source");
            let supplied = placeholders(value);
            let mut expected = placeholders(source);
            // English can leave the count implicit for one item, while a
            // translated `one` form also covers counts such as Russian 21.
            if key
                .strip_suffix(".one")
                .is_some_and(|stem| english.contains_key(format!("{stem}.other").as_str()))
                && supplied.contains("n")
            {
                expected.insert("n".into());
            }
            assert_eq!(
                supplied,
                expected,
                "{}: {key} has different placeholders from English",
                locale.tag()
            );
        }
    }
}

fn plural_stems() -> BTreeSet<&'static str> {
    catalog(Locale::En)
        .keys()
        .filter_map(|key| key.strip_suffix(".one"))
        .filter(|stem| catalog(Locale::En).contains_key(format!("{stem}.other").as_str()))
        .collect()
}

#[test]
fn plural_strings_have_all_required_categories_and_counts() {
    for locale in Locale::ALL {
        // A singular-looking form can represent 0, 21, 101, etc. Use all
        // vendored CLDR integer examples to catch hidden counts as new
        // languages are registered, including cases beyond small integers.
        let one_requires_number = plurals::INTEGER_SAMPLES
            .iter()
            .any(|(_, n, _)| *n != 1 && locale.plural_category(*n) == "one");
        for stem in plural_stems() {
            for category in locale.plural_categories() {
                let key = format!("{stem}.{category}");
                let value = catalog(locale)
                    .get(key.as_str())
                    .unwrap_or_else(|| panic!("{}: missing {key}", locale.tag()));
                assert!(
                    value.contains("{n}") || (*category == "one" && !one_requires_number),
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
        for (key, value) in catalog(locale) {
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
    assert_eq!(Locale::from_tag("ja-JP"), Some(Locale::Ja));
    assert_eq!(Locale::from_tag("ja_JP.UTF-8"), Some(Locale::Ja));
    assert_eq!(Locale::from_tag("en-GB"), Some(Locale::En));
    assert_eq!(Locale::from_tag("fr-FR"), Some(Locale::Fr));
    assert_eq!(Locale::from_tag("es_MX.UTF-8"), Some(Locale::Es));
    assert_eq!(Locale::from_tag("ru-RU"), Some(Locale::Ru));
    assert_eq!(Locale::from_tag("zz-ZZ"), None);
    assert_eq!(Locale::from_tag("C"), None);
    assert_eq!(Locale::from_tag("POSIX"), None);
    assert_eq!(Locale::from_tag(""), None);
}

#[test]
fn the_first_language_we_have_wins() {
    assert_eq!(Locale::negotiate(["fr-FR", "sv-SE", "en"]), Locale::Fr);
    // Nothing on the list is a language Schist has.
    assert_eq!(Locale::negotiate(["zz-ZZ", "qaa"]), Locale::En);
    assert_eq!(Locale::negotiate(Vec::<String>::new()), Locale::En);
    assert_eq!(Locale::negotiate(["de", "sv"]), Locale::De);
    assert_eq!(Locale::negotiate(["zz", "ja", "en"]), Locale::Ja);
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
    // Japanese marks no number either, so one string serves both.
    set_locale(Locale::Ja);
    assert_eq!(
        tn("common.n_layers", 1),
        tn("common.n_layers", 7).replace('7', "1")
    );
    set_locale(Locale::De);
    assert_eq!(tn("common.n_layers", 1), "1 Ebene");
    assert_eq!(tn("common.n_layers", 5), "5 Ebenen");
}

#[test]
fn new_locales_choose_non_english_cardinal_categories() {
    let _guard = with_locale(Locale::Fr);
    assert_eq!(Locale::Fr.plural_category(0), "one");
    assert_eq!(Locale::Fr.plural_category(1_000_000), "many");
    assert_eq!(tn("common.n_layers", 0), "0 calque");
    set_locale(Locale::Es);
    assert_eq!(Locale::Es.plural_category(0), "other");
    assert_eq!(Locale::Es.plural_category(1_000_000), "many");
    set_locale(Locale::Ru);
    for (n, suffix) in [
        (1, "слой"),
        (2, "слоя"),
        (5, "слоёв"),
        (11, "слоёв"),
        (21, "слой"),
        (22, "слоя"),
        (25, "слоёв"),
        (u64::MAX, "слоёв"),
    ] {
        assert_eq!(tn("common.n_layers", n), format!("{n} {suffix}"));
    }
    // These English `one` forms omit {n}; Russian still needs the count
    // when the same category describes 21 selected photos or documents.
    assert_eq!(
        tn("library.bucket.starts_with", 21),
        "В подборку будет добавлена 21 выбранная фотография."
    );
    assert_eq!(
        tn("workspace.recovery.recovered", 21),
        "Восстановлен 21 документ из предыдущего сеанса"
    );
}

#[test]
fn arabic_counts_cover_six_categories_and_keep_the_displayed_number() {
    let _guard = with_locale(Locale::Ar);
    assert!(Locale::Ar.is_rtl());
    assert_eq!(Locale::Ar.script(), "Arab");
    assert_eq!(Locale::from_tag("ar_SA.UTF-8"), Some(Locale::Ar));
    for (n, category, rendered) in [
        (0, "zero", "0 طبقة"),
        (1, "one", "طبقة واحدة (1)"),
        (2, "two", "طبقتان (2)"),
        (3, "few", "3 طبقات"),
        (10, "few", "10 طبقات"),
        (11, "many", "11 طبقة"),
        (99, "many", "99 طبقة"),
        (100, "other", "100 طبقة"),
        (102, "other", "102 طبقة"),
        (103, "few", "103 طبقات"),
    ] {
        assert_eq!(Locale::Ar.plural_category(n), category);
        assert_eq!(tn("common.n_layers", n), rendered);
    }
}

#[test]
fn every_registered_locale_round_trips_and_has_metadata() {
    let mut tags = BTreeSet::new();
    for locale in Locale::ALL {
        assert!(tags.insert(locale.tag()), "duplicate locale tag");
        assert_eq!(Locale::from_tag(locale.tag()), Some(locale));
        assert_eq!(
            Locale::from_tag(&locale.tag().to_ascii_uppercase()),
            Some(locale)
        );
        assert!(!locale.native_name().is_empty());
        assert_eq!(locale.script().len(), 4);
        assert!(locale
            .plural_categories()
            .contains(&locale.plural_category(u64::MAX)));
    }
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

/// Check the same locale-to-font mapping the browser manifest uses. Every
/// visible catalog character must be drawable by one of the fetched faces.
#[test]
fn the_web_fonts_cover_their_catalogs() {
    let fonts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/fonts");
    let mapping: BTreeMap<String, Vec<String>> =
        serde_json::from_str(&std::fs::read_to_string(fonts.join("locales.json")).unwrap())
            .unwrap();
    let buffers: Vec<_> = std::fs::read_dir(&fonts)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "ttf" || ext == "otf")
        })
        .map(|path| {
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            assert!(
                name == "IBMPlexSans-Regular.ttf" || mapping.contains_key(&name),
                "{name} is missing its font locale mapping"
            );
            (name, std::fs::read(path).unwrap())
        })
        .collect();
    for file in mapping.keys() {
        assert!(
            fonts.join(file).is_file(),
            "missing {file}; run tools/web-i18n-fonts.py"
        );
    }
    let faces: Vec<_> = buffers
        .iter()
        .map(|(name, bytes)| {
            (
                name,
                ttf_parser::Face::parse(bytes, 0).expect("a parseable font"),
            )
        })
        .collect();
    for locale in Locale::ALL {
        let selected: Vec<_> = faces
            .iter()
            .filter(|(name, _)| {
                mapping
                    .get(name.as_str())
                    .is_none_or(|tags| tags.iter().any(|tag| tag == locale.tag()))
            })
            .collect();
        assert!(!selected.is_empty(), "{} has no web fonts", locale.tag());
        let mut missing = BTreeSet::new();
        for value in catalog(locale).values() {
            for c in value.chars() {
                // Whitespace, bidi controls, joiners, and variation selectors
                // influence layout but don't need their own visible glyph.
                let invisible = c.is_whitespace()
                    || matches!(c as u32,
                    0x00AD | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F |
                    0xFE00..=0xFE0F | 0xFEFF | 0xE0100..=0xE01EF);
                if !invisible
                    && !selected
                        .iter()
                        .any(|(_, face)| face.glyph_index(c).is_some())
                {
                    missing.insert(c);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "{}: web fonts lack {}",
            locale.tag(),
            missing.iter().collect::<String>()
        );
    }
}
