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
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let root =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("locales");
    println!("cargo::rerun-if-changed=locales.tsv");
    println!("cargo::rerun-if-changed=build.rs");
    let registry = fs::read_to_string("locales.tsv").expect("reading locales.tsv");
    let locales: Vec<Vec<&str>> = registry
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 5, "invalid locale record: {line}");
            assert!(fields[0]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-'));
            assert!(fields[1].chars().all(|c| c.is_ascii_alphanumeric()));
            assert!(matches!(fields[3], "ltr" | "rtl"));
            fields
        })
        .collect();
    assert_eq!(
        locales.first().map(|row| row[0]),
        Some("en"),
        "English must be first"
    );
    let mut generated = String::from(
        "/// A language Schist's chrome is available in.\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n\
         #[repr(usize)]\n\
         pub enum Locale {\n",
    );
    for row in &locales {
        writeln!(generated, "    {},", row[1]).unwrap();
    }
    generated.push_str("}\nimpl Locale {\n");
    writeln!(
        generated,
        "    pub const ALL: [Self; {}] = [",
        locales.len()
    )
    .unwrap();
    for row in &locales {
        writeln!(generated, "        Self::{},", row[1]).unwrap();
    }
    generated.push_str("    ];\n}\n");
    writeln!(
        generated,
        "const METADATA: [(&str, &str, bool, &str); {}] = [",
        locales.len()
    )
    .unwrap();
    for row in &locales {
        writeln!(
            generated,
            "    ({:?}, {:?}, {}, {:?}),",
            row[0],
            row[2],
            row[3] == "rtl",
            row[4]
        )
        .unwrap();
    }
    generated.push_str("];\n");
    writeln!(generated, "const SOURCES: [&str; {}] = [", locales.len()).unwrap();
    for row in &locales {
        let locale = row[0];
        let dir = root.join(locale);
        // Watch each shipped directory, including newly added files, without
        // rebuilding for edits to unregistered translation drafts.
        println!("cargo::rerun-if-changed=locales/{locale}");
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
        writeln!(
            generated,
            "    include_str!(concat!(env!(\"OUT_DIR\"), \"/{locale}.lang\")),"
        )
        .unwrap();
    }
    generated.push_str("];\n");
    fs::write(out.join("locales.rs"), generated).expect("writing the locale registry");
}
