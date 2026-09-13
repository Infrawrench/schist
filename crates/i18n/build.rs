//! Stitch each locale's catalog files into one, so the crate embeds a
//! single `include_str!` per language.
//!
//! The catalogs live in `locales/<locale>/*.lang`, one file per area of
//! the app (`menu.lang`, `tools.lang`, `filters.lang`, …) so that adding
//! strings for one panel does not mean editing one enormous file, and so
//! two people adding strings for different panels never conflict. This
//! joins them in name order; the parser does not care where a key came
//! from, and the tests check that no key is defined twice.

use std::env;
use std::fs;
use std::path::PathBuf;

/// The locales Schist ships. Kept in step with `Locale::ALL` in
/// `src/lib.rs`; the test there fails if a directory is missing.
const LOCALES: &[&str] = &["en", "sv", "de", "zh-Hans", "ja"];

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let root =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("locales");
    // A directory: cargo watches everything under it, so a new `.lang`
    // file is picked up as well as an edit to an existing one.
    println!("cargo::rerun-if-changed=locales");
    println!("cargo::rerun-if-changed=build.rs");
    for locale in LOCALES {
        let dir = root.join(locale);
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("reading {}: {err}", dir.display()))
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "lang"))
            .collect();
        files.sort();
        let mut joined = String::new();
        for file in files {
            let name = file.file_name().unwrap_or_default().to_string_lossy();
            let text = fs::read_to_string(&file)
                .unwrap_or_else(|err| panic!("reading {}: {err}", file.display()));
            joined.push_str(&format!("# ==== {name} ====\n"));
            joined.push_str(&text);
            if !text.ends_with('\n') {
                joined.push('\n');
            }
        }
        fs::write(out.join(format!("{locale}.lang")), joined).expect("writing the joined catalog");
    }
}
