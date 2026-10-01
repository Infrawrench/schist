//! Cross-checks against the system `zip` and `unzip`.
//!
//! The container in [`crate::container`] is hand-written, and a
//! hand-written ZIP reader is exactly the kind of code that passes its own
//! tests and then fails on a file it has never seen. These tests use the
//! system's own implementations as the other side of the comparison:
//! `zip` writes a package this crate must read, and `unzip` reads a package
//! this crate writes.
//!
//! Both are skipped, not failed, when the tools are absent, so the suite
//! still runs on a machine without them -- but a skip is visible, and on
//! any machine that has them these are the tests that give the container
//! its confidence.

use std::path::{Path, PathBuf};
use std::process::Command;

use schist_codec_idml::container::{self};

/// Whether a tool is on the PATH.
fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("-v")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// A scratch directory, emptied first so a previous run cannot leak in.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("schist-idml-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn parts() -> Vec<(String, Vec<u8>)> {
    vec![
        (
            container::MIMETYPE_PART.to_string(),
            container::MIMETYPE.as_bytes().to_vec(),
        ),
        (
            "META-INF/container.xml".to_string(),
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0">
  <rootfiles>
    <rootfile full-path="designmap.xml" media-type="application/vnd.adobe.indesign-idml-package"/>
  </rootfiles>
</container>"#
                .to_vec(),
        ),
        (
            "designmap.xml".to_string(),
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
  <idPkg:Spread Self="spread_1"><idPkg:Page Self="Page_1"/></idPkg:Spread>
</Document>"#
                .to_vec(),
        ),
        // A part with real text, accents and an em dash, to prove the
        // encoding survives a round trip through both implementations.
        (
            "Stories/story_1.xml".to_string(),
            "<Story>Grüße, monde — 日本語</Story>".as_bytes().to_vec(),
        ),
        // And a binary part, because not everything in a package is XML.
        (
            "Links/link_1.png".to_string(),
            (0..=255u8).cycle().take(4096).collect(),
        ),
    ]
}

/// Write the parts into `dir`, returning it.
fn write_parts(dir: &Path, parts: &[(String, Vec<u8>)]) -> PathBuf {
    for (name, bytes) in parts {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a part directory");
        }
        std::fs::write(path, bytes).expect("writing a scratch part");
    }
    dir.to_path_buf()
}

/// Zip `dir` with the system tool, writing the archive *outside* it.
///
/// The archive has to sit outside the tree: `zip -r archive .` would
/// otherwise archive the archive, and the test would pass on a package
/// containing more entries than it should.
fn system_zip(dir: &Path, archive: &Path, flag: &str) {
    let status = Command::new("zip")
        // `-D` omits directory entries, so the archive holds exactly the
        // parts and the entry count is meaningful.
        .args([flag, "-X", "-D", "-r", "-q"])
        .arg(archive)
        .arg(".")
        .current_dir(dir)
        .status()
        .expect("running zip");
    assert!(status.success(), "zip {flag} failed");
}

#[test]
fn a_package_the_system_zip_wrote_this_crate_can_read() {
    if !have("zip") {
        eprintln!("skipping: no system `zip`");
        return;
    }
    let outer = scratch("read-system-zip");
    let dir = write_parts(&outer.join("parts"), &parts());

    // `zip -X` drops extra fields, and `-0`/`-9` pick the method, so the
    // reader is tested against both stored and deflated entries.
    for (flag, label) in [("-0", "stored"), ("-9", "deflated")] {
        let archive = outer.join(format!("system{flag}.idml"));
        system_zip(&dir, &archive, flag);

        let bytes = std::fs::read(&archive).expect("reading the archive zip wrote");
        let package = container::read(&bytes).expect("a package written by the system zip");
        assert_eq!(package.len(), parts().len(), "{label}");
        for (name, expected) in parts() {
            assert_eq!(
                package
                    .get(&name)
                    .unwrap_or_else(|| panic!("{name} missing ({label})")),
                expected.as_slice(),
                "{name} differs ({label})"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&outer);
}

#[test]
fn a_package_this_crate_wrote_the_system_unzip_can_read() {
    if !have("unzip") {
        eprintln!("skipping: no system `unzip`");
        return;
    }
    let dir = scratch("write-system-unzip");
    let archive = dir.join("schist.idml");
    std::fs::write(&archive, container::write(&parts())).expect("writing the archive");

    // `-t` tests the whole archive's CRCs without extracting.
    let test = Command::new("unzip")
        .args(["-t", "-qq"])
        .arg(&archive)
        .output()
        .expect("running unzip");
    assert!(
        test.status.success(),
        "unzip rejected the package:\n{}\n{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );

    // And extracting reproduces every part byte for byte.
    let out = dir.join("out");
    let extract = Command::new("unzip")
        .args(["-qq", "-o"])
        .arg(&archive)
        .arg("-d")
        .arg(&out)
        .output()
        .expect("running unzip");
    assert!(
        extract.status.success(),
        "unzip could not extract:\n{}",
        String::from_utf8_lossy(&extract.stderr)
    );
    for (name, expected) in parts() {
        let extracted = std::fs::read(out.join(&name))
            .unwrap_or_else(|error| panic!("{name} not extracted: {error}"));
        assert_eq!(extracted, expected, "{name} differs after extraction");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_system_agrees_that_mimetype_comes_first_and_is_stored() {
    if !have("unzip") {
        eprintln!("skipping: no system `unzip`");
        return;
    }
    // The scratch directory name must not itself contain "mimetype": the
    // verbose listing is searched for that word, and the archive header
    // line names the file.
    let dir = scratch("opc-order");
    let archive = dir.join("order.idml");
    // Deliberately insert `mimetype` last, so only the writer can put it
    // first.
    let mut shuffled = parts();
    if let Some(at) = shuffled
        .iter()
        .position(|(name, _)| name == container::MIMETYPE_PART)
    {
        let mimetype = shuffled.remove(at);
        shuffled.push(mimetype);
    }
    std::fs::write(&archive, container::write(&shuffled)).expect("writing the archive");

    // `-Z1` lists entry names in archive order, one per line, with no
    // header to parse around.
    let listing = Command::new("unzip")
        .args(["-Z1", archive.to_str().unwrap()])
        .output()
        .expect("running unzip");
    let text = String::from_utf8_lossy(&listing.stdout);
    let names: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    assert!(!names.is_empty(), "unzip listed no entries:\n{text}");
    assert_eq!(
        names[0],
        container::MIMETYPE_PART,
        "the first entry must be mimetype:\n{text}"
    );

    // `-v` reports the method, and "Stored" is what an OPC reader needs.
    let verbose = Command::new("unzip")
        .args(["-v", archive.to_str().unwrap()])
        .output()
        .expect("running unzip");
    let text = String::from_utf8_lossy(&verbose.stdout);
    let mimetype_line = text
        .lines()
        .find(|line| line.contains(container::MIMETYPE_PART))
        .unwrap_or_else(|| panic!("no mimetype line in:\n{text}"));
    assert!(
        mimetype_line.contains("Stored"),
        "mimetype must be stored, not deflated:\n{mimetype_line}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_package_written_by_the_system_survives_our_own_round_trip() {
    // Belt and braces: a package read from a system-written archive and
    // written back out is still readable, which is what a resave does.
    if !have("zip") || !have("unzip") {
        eprintln!("skipping: needs both zip and unzip");
        return;
    }
    let outer = scratch("resave");
    let dir = write_parts(&outer.join("parts"), &parts());
    let archive = outer.join("in.idml");
    system_zip(&dir, &archive, "-9");

    let read = container::open(&std::fs::read(&archive).expect("reading")).expect("reading it");
    let written = container::write(&read.into_parts());
    let reread = container::open(&written).expect("re-reading our own output");
    assert_eq!(reread.len(), parts().len());
    for (name, expected) in parts() {
        assert_eq!(reread.get(&name).unwrap(), expected.as_slice(), "{name}");
    }

    // And the system still accepts what we wrote.
    let out = dir.join("out.idml");
    std::fs::write(&out, written).expect("writing");
    let test = Command::new("unzip")
        .args(["-t", "-qq"])
        .arg(&out)
        .output()
        .expect("running unzip");
    assert!(
        test.status.success(),
        "unzip rejected the resaved package:\n{}",
        String::from_utf8_lossy(&test.stderr)
    );
    let _ = std::fs::remove_dir_all(&outer);
}
