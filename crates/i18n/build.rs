//! Join and compress each locale's catalog using a shared preset dictionary,
//! so the crate embeds compressed bytes and inflates only languages in use.
//!
//! The catalogs live in `locales/<locale>/*.lang`, one file per area of
//! the app (`menu.lang`, `tools.lang`, `filters.lang`, …) so that adding
//! strings for one panel does not mean editing one enormous file, and so
//! two people adding strings for different panels never conflict. This
//! joins them in name order; the parser does not care where a key came
//! from, and the tests check that no key is defined twice.

use flate2::{write::ZlibEncoder, Compress, Compression};
use std::collections::BTreeMap;
use std::env;
use std::fmt::Write;
use std::fs;
use std::io::Write as IoWrite;
use std::path::PathBuf;

const DICTIONARY_BYTES: usize = 32 * 1024;

/// DEFLATE can refer back only 32 KiB, so learn from the beginning of each
/// catalog, where a preset dictionary can help. Repeated keys and values
/// make useful matches across languages; estimated savings rank the candidates.
/// Ordered maps and lexical tie-breaking make the result reproducible.
fn dictionary(sources: &[String]) -> Vec<u8> {
    let mut counts = BTreeMap::<&str, usize>::new();
    for source in sources {
        let mut end = source.len().min(DICTIONARY_BYTES);
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        for line in source[..end].lines() {
            if line.trim_start().starts_with('#') {
                continue;
            }
            if let Some(separator) = line.find('=') {
                for text in [&line[..=separator], line[separator + 1..].trim()] {
                    if !text.is_empty() {
                        *counts.entry(text).or_default() += 1;
                    }
                }
            }
        }
    }
    let mut ranked: Vec<_> = counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(text, count)| ((count - 1) * text.len(), text))
        .collect();
    ranked.sort_unstable();
    let mut selected = Vec::new();
    let mut size = 0;
    for (_, text) in ranked.into_iter().rev() {
        if size + text.len() < DICTIONARY_BYTES {
            selected.push(text);
            size += text.len() + 1;
        }
    }
    let mut dictionary = Vec::with_capacity(size);
    // Put the most useful matches last, where they stay in the window longest.
    for text in selected.into_iter().rev() {
        dictionary.extend_from_slice(text.as_bytes());
        dictionary.push(b'\n');
    }
    dictionary
}

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
    let mut sources = Vec::with_capacity(locales.len());
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
        sources.push(joined);
    }
    // The library protocol has no process-global active locale or lazy caches.
    let mut english = BTreeMap::new();
    for line in sources[0].lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let mut decoded = String::new();
            let mut chars = value.trim().chars();
            while let Some(c) = chars.next() {
                if c != '\\' {
                    decoded.push(c);
                    continue;
                }
                match chars.next() {
                    Some('n') => decoded.push('\n'),
                    Some('\\') => decoded.push('\\'),
                    Some(c) => {
                        decoded.push('\\');
                        decoded.push(c);
                    }
                    None => decoded.push('\\'),
                }
            }
            english.insert(key.trim(), decoded);
        }
    }
    let mut table = String::from("const ENGLISH: &[(&str, &str)] = &[\n");
    for (key, value) in english {
        writeln!(table, "({key:?}, {value:?}),").unwrap();
    }
    table.push_str("];\n");
    fs::write(out.join("english.rs"), table).expect("writing immutable English catalog");
    // Gzip cannot carry a preset dictionary. The zlib wrapper records its
    // Adler-32 ID and a checksum of each catalog, checked during inflation.
    let dictionary = dictionary(&sources);
    fs::write(out.join("locales.dict"), &dictionary).expect("writing the shared locale dictionary");
    generated.push_str(
        "const DICTIONARY: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/locales.dict\"));\n",
    );
    writeln!(
        generated,
        "const SOURCES: [(&[u8], usize); {}] = [",
        locales.len()
    )
    .unwrap();
    for (row, source) in locales.iter().zip(&sources) {
        let locale = row[0];
        let mut compressor = Compress::new(Compression::best(), true);
        compressor
            .set_dictionary(&dictionary)
            .expect("setting the locale dictionary");
        let mut encoder = ZlibEncoder::new_with_compress(Vec::new(), compressor);
        encoder
            .write_all(source.as_bytes())
            .expect("compressing the catalog");
        let compressed = encoder.finish().expect("finishing the compressed catalog");
        fs::write(out.join(format!("{locale}.lang.zlib")), compressed)
            .expect("writing the compressed catalog");
        writeln!(
            generated,
            "    (include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{locale}.lang.zlib\")), {}),",
            source.len()
        )
        .unwrap();
    }
    generated.push_str("];\n");
    fs::write(out.join("locales.rs"), generated).expect("writing the locale registry");
}
