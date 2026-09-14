//! Schist's user-interface strings.
//!
//! Every label, title, menu entry and message a person reads comes
//! through here. The English text lives in `locales/en/*.lang`, the
//! translations beside it, and code names a string by key:
//!
//! ```
//! use schist_i18n::{t, tf, tn};
//!
//! let cancel: &'static str = t("common.cancel");
//! let saved = tf!("common.saved_as", name = "moss.psd");
//! let layers = tn("common.n_layers", 3);
//! ```
//!
//! [`t`] returns a `&'static str`, which is what makes the rest of the
//! app cheap to localise: the plugin traits declare a tool's `name()` as
//! `&'static str`, the menu model holds `&'static str` labels, and none
//! of them had to change. Catalogs are compressed at build time with a
//! shared dictionary, then inflated once on first use and kept for the
//! process's lifetime, so translations remain static slices.
//!
//! # Which language
//!
//! [`init`] asks the operating system for the languages the user
//! prefers, in order, and picks the first one Schist has: on macOS and
//! iOS the languages in System Settings (including a per-app choice),
//! on Windows the display language, on Linux `LANGUAGE`, `LC_ALL`,
//! `LC_MESSAGES` and `LANG`, on Android the device (or per-app) locale,
//! and in a browser `navigator.languages`. Anything unmatched falls back
//! to English. `SCHIST_LANG=sv` in the environment overrides the lot on
//! a native build, which is how a translation is checked without
//! changing the system.
//!
//! The choice is made once at startup. Changing the system language
//! while Schist runs takes effect on the next launch, as it does in most
//! native apps.
//!
//! # The catalog format
//!
//! One `key = value` per line. `#` starts a comment; blank lines are
//! ignored. Values run to the end of the line, trimmed; `\n` and `\\`
//! are the two escapes. A placeholder is `{name}` and is filled by
//! [`tf`]; count strings provide the language's integer cardinal variants
//! for [`tn`] (`zero`, `one`, `two`, `few`, `many`, `other`), where `{n}`
//! is the count. Chinese and Japanese need only `.other`. A key that merely ends
//! in `.other` — `filter.category.other`, a heading — is an ordinary
//! string; it is the `.one` beside it that makes a pair.
//!
//! Keys are `area.item` in snake case — `menu.file.new`,
//! `tool.move.name`, `filter.gaussian_blur.name`, `dialog.new_doc.title`.
//! The area names the `.lang` file the key lives in, so `menu.*` is in
//! `menu.lang`, and `common.*` holds what every area shares: OK, Cancel,
//! Width, Height, the file-type names.
//!
//! A missing key is a bug, not a condition: [`t`] falls back to the
//! English text, and past that to the key itself, logging once. The
//! tests in this crate check that every translation has exactly the keys
//! English has, with the same placeholders, and that every key the
//! source tree asks for exists.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

mod plurals;

// The registry and embedded catalogs are generated together, so their indices
// cannot drift when a language is added.
include!(concat!(env!("OUT_DIR"), "/locales.rs"));

impl Locale {
    /// The BCP 47 tag, also the catalog directory's name.
    pub fn tag(self) -> &'static str {
        METADATA[self.index()].0
    }

    /// The language's own name for itself.
    pub fn native_name(self) -> &'static str {
        METADATA[self.index()].1
    }

    /// Whether the language's default script is written right to left.
    pub fn is_rtl(self) -> bool {
        METADATA[self.index()].2
    }

    /// The default ISO 15924 script code, for font selection.
    pub fn script(self) -> &'static str {
        METADATA[self.index()].3
    }

    /// Cardinal categories for this language's integer counts, plus `other`
    /// as the required fallback even when only fractions would select it.
    pub fn plural_categories(self) -> &'static [&'static str] {
        plurals::categories(self.language())
    }

    /// Whether this language has a `one` cardinal category.
    pub fn has_singular(self) -> bool {
        self.plural_categories().contains(&"one")
    }

    /// The Unicode CLDR cardinal category for an integer count.
    pub fn plural_category(self, n: u64) -> &'static str {
        plurals::category(self.language(), n)
    }

    fn language(self) -> &'static str {
        self.tag().split('-').next().unwrap()
    }

    fn index(self) -> usize {
        self as usize
    }

    /// Match BCP 47 and POSIX tags, including common legacy language codes.
    /// Regions use the language's default catalog; `zh` uses Simplified Chinese.
    pub fn from_tag(tag: &str) -> Option<Locale> {
        let language = tag
            .trim()
            .split(['-', '_', '.', '@'])
            .next()?
            .to_ascii_lowercase();
        let language = match language.as_str() {
            "iw" => "he",
            "in" => "id",
            "ji" => "yi",
            "jw" => "jv",
            "mo" => "ro",
            "fil" => "tl",
            other => other,
        };
        Self::ALL
            .into_iter()
            .find(|locale| locale.language() == language)
    }

    /// The first of the user's preferred languages Schist has, or
    /// English when none of them is.
    pub fn negotiate<I, S>(preferred: I) -> Locale
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        preferred
            .into_iter()
            .find_map(|tag| Locale::from_tag(tag.as_ref()))
            .unwrap_or(Locale::En)
    }
}

type Catalog = HashMap<&'static str, &'static str>;

fn catalog(locale: Locale) -> &'static Catalog {
    static CATALOGS: [OnceLock<Catalog>; Locale::ALL.len()] =
        [const { OnceLock::new() }; Locale::ALL.len()];
    CATALOGS[locale.index()].get_or_init(|| parse(source(locale)).collect())
}

fn source(locale: Locale) -> &'static str {
    // Static storage keeps the text alive, and OnceLock shares one allocation
    // per used language even when lookups race on different threads.
    static DECOMPRESSED: [OnceLock<String>; Locale::ALL.len()] =
        [const { OnceLock::new() }; Locale::ALL.len()];
    DECOMPRESSED[locale.index()]
        .get_or_init(|| {
            let (compressed, len) = SOURCES[locale.index()];
            decompress(compressed, len, DICTIONARY)
                .unwrap_or_else(|err| panic!("decompressing the {} catalog: {err}", locale.tag()))
        })
        .as_str()
}

fn decompress(compressed: &[u8], len: usize, dictionary: &[u8]) -> std::io::Result<String> {
    use flate2::{Decompress, FlushDecompress, Status};
    use std::io::{Error, ErrorKind};

    let mut decoder = Decompress::new(true);
    let mut output = vec![0; len];
    let status = match decoder.decompress(compressed, &mut output, FlushDecompress::Finish) {
        Err(err) if err.needs_dictionary().is_some() => {
            decoder.set_dictionary(dictionary)?;
            decoder.decompress(
                &compressed[decoder.total_in() as usize..],
                &mut output,
                FlushDecompress::Finish,
            )?
        }
        result => result?,
    };
    if status != Status::StreamEnd
        || decoder.total_in() != compressed.len() as u64
        || decoder.total_out() != len as u64
    {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "invalid catalog length or incomplete stream",
        ));
    }
    String::from_utf8(output).map_err(|err| Error::new(ErrorKind::InvalidData, err))
}

/// The active locale's index into `SOURCES`; English until `init` runs.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Choose the language from the operating system's preferences, or from
/// `SCHIST_LANG` when that is set. Call once, first thing at startup;
/// every lookup before it answers in English.
pub fn init() {
    init_from_tags(system_tags());
}

/// Choose the language from an explicit preference list, highest first,
/// still honouring `SCHIST_LANG`. For a platform that knows the user's
/// choice better than `sys-locale` does — Android's activity
/// configuration carries a per-app language the system properties do
/// not — hand its answer here, followed by [`system_tags`].
pub fn init_from_tags<I, S>(preferred: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let locale = match override_tag() {
        Some(tag) => Locale::from_tag(&tag).unwrap_or_else(|| {
            log::warn!("SCHIST_LANG={tag} names no language Schist has; using the system's");
            Locale::negotiate(preferred)
        }),
        None => Locale::negotiate(preferred),
    };
    set_locale(locale);
    log::info!("language: {}", locale.tag());
}

/// Use this locale, whatever the system says. For tests and for anything
/// that must speak English regardless — the headless MCP server, whose
/// tool descriptions are read by a model, not a person.
pub fn set_locale(locale: Locale) {
    ACTIVE.store(locale.index(), Ordering::Release);
}

/// The language in use.
pub fn locale() -> Locale {
    Locale::ALL[ACTIVE.load(Ordering::Acquire)]
}

/// The languages the operating system says the user prefers, in order.
pub fn system_tags() -> Vec<String> {
    sys_locale::get_locales().collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn override_tag() -> Option<String> {
    std::env::var("SCHIST_LANG")
        .ok()
        .filter(|tag| !tag.trim().is_empty())
}

#[cfg(target_arch = "wasm32")]
fn override_tag() -> Option<String> {
    None
}

/// The string for `key` in the active language.
///
/// Falls back to English, and past that to the key itself, so a missing
/// translation never blanks a control. Both fallbacks are bugs the
/// crate's tests catch; the second is also logged, once per key.
pub fn t(key: &str) -> &'static str {
    if let Some(value) = catalog(locale()).get(key) {
        return value;
    }
    if let Some(value) = catalog(Locale::En).get(key) {
        return value;
    }
    missing(key)
}

/// The string for `key` in one particular language, whatever is active.
///
/// For matching text that arrives untranslated against a catalog entry:
/// a Photoshop plug-in names its Filter-menu category in English, and
/// the menu that files it under the translated heading compares against
/// the English form.
pub fn t_in(locale: Locale, key: &str) -> &'static str {
    if let Some(value) = catalog(locale).get(key) {
        return value;
    }
    if let Some(value) = catalog(Locale::En).get(key) {
        return value;
    }
    missing(key)
}

/// The string for `key` with its `{name}` placeholders filled from
/// `args`. Prefer the [`tf!`] macro, which spells the pairs for you.
pub fn tf(key: &str, args: &[(&str, &dyn Display)]) -> String {
    fill(t(key), args)
}

/// The string for an integer count, using the language's CLDR cardinal
/// category (`zero`, `one`, `two`, `few`, `many`, or `other`).
pub fn tn(key: &str, n: u64) -> String {
    tnf(key, n, &[])
}

/// [`tn`] with further placeholders besides `{n}`.
pub fn tnf(key: &str, n: u64, args: &[(&str, &dyn Display)]) -> String {
    let language = locale();
    let full = format!("{key}.{}", language.plural_category(n));
    let other = format!("{key}.other");
    // Stay in the selected language before trying English; an English `one`
    // entry must never replace a translated count-neutral `other` entry.
    let text = catalog(language)
        .get(full.as_str())
        .or_else(|| catalog(language).get(other.as_str()))
        .copied()
        .unwrap_or_else(|| {
            t_in(
                Locale::En,
                &format!("{key}.{}", Locale::En.plural_category(n)),
            )
        });
    let mut with_n: Vec<(&str, &dyn Display)> = Vec::with_capacity(args.len() + 1);
    with_n.push(("n", &n));
    with_n.extend_from_slice(args);
    fill(text, &with_n)
}

/// Whether the active language, or English, has a string for `key`.
pub fn has(key: &str) -> bool {
    catalog(locale()).contains_key(key) || catalog(Locale::En).contains_key(key)
}

/// A list of strings for a list of keys, with the `'static` lifetime the
/// plugin API's choice lists carry.
///
/// A tool's dropdown declares its entries as `&'static [&'static str]`,
/// which a `const` cannot translate at runtime. This
/// looks each key up and keeps the list for the life of the process;
/// asking again for the same keys in the same language returns the same
/// list, so the memory is bounded by the number of distinct lists.
pub fn choices(keys: &'static [&'static str]) -> &'static [&'static str] {
    // The cache key is the list's address and length: a
    // `&'static [&'static str]` never moves, so they identify it for the
    // life of the process. Kept as an integer, which is all it is used as.
    type Entry = (usize, usize, usize, &'static [&'static str]);
    static LISTS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
    let language = locale();
    let locale = language.index();
    let (ptr, len) = (keys.as_ptr() as usize, keys.len());
    let mut lists = LISTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = lists
        .iter()
        .find(|(l, p, n, _)| *l == locale && *p == ptr && *n == len)
    {
        return hit.3;
    }
    let list: &'static [&'static str] = Box::leak(
        keys.iter()
            .map(|key| t_in(language, key))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    lists.push((locale, ptr, len, list));
    list
}

/// Fill `{name}` placeholders. Unknown ones are left as written, so a
/// stray brace in a translation is visible rather than silently eaten.
fn fill(text: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if !after[..close].contains('{') => {
                let name = &after[..close];
                match args.iter().find(|(k, _)| *k == name) {
                    Some((_, value)) => {
                        use std::fmt::Write as _;
                        let _ = write!(out, "{value}");
                    }
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A key with no string in any language: report it once and hand back
/// the key, kept for the process's life so the return can be `'static`.
fn missing(key: &str) -> &'static str {
    static MISSING: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut seen = MISSING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = seen.iter().find(|k| **k == key) {
        return hit;
    }
    log::warn!("no string for {key:?} in any language");
    let leaked: &'static str = Box::leak(key.to_string().into_boxed_str());
    seen.push(leaked);
    leaked
}

/// Parse a catalog into its pairs. Values are slices of the source
/// unless they carry an escape, in which case the unescaped text is
/// kept for the life of the process (once, at first use).
fn parse(source: &'static str) -> impl Iterator<Item = (&'static str, &'static str)> {
    source.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() {
            return None;
        }
        let value = if value.contains('\\') {
            Box::leak(unescape(value).into_boxed_str())
        } else {
            value
        };
        Some((key, value))
    })
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// [`tf`] with the placeholders named inline:
/// `tf!("common.saved_as", name = path.display())`.
#[macro_export]
macro_rules! tf {
    ($key:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::tf($key, &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),*])
    };
}

/// [`tnf`] with the placeholders named inline:
/// `tn!("common.n_of_m", n, m = total)`.
#[macro_export]
macro_rules! tn {
    ($key:expr, $n:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::tnf($key, $n, &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),*])
    };
}

#[cfg(test)]
mod tests;
