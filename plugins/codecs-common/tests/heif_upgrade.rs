#![cfg(not(any(target_arch = "wasm32", target_os = "ios")))]

use schist_codecs_common::heif::{self, Unavailable};

// A separate test executable keeps the managed-directory override and
// library cache isolated from real HEIC import tests.
#[test]
fn heif_outdated_managed_library_still_offers_the_security_update() {
    let Some(managed) = heif::managed_library() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("schist-heif-upgrade-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("SCHIST_LIBHEIF_DIR", &dir);
    let path = dir.join(managed.library.name);
    let old_bytes = b"previously downloaded library";
    std::fs::write(&path, old_bytes).unwrap();

    let error = anyhow::Error::new(Unavailable::NoLibrary {
        details: "libheif 1.23.2 rejected".into(),
    })
    .context("generating a gallery thumbnail");
    assert!(!heif::managed_installed());
    assert!(heif::download_would_help(&error));
    assert!(!heif::download_would_help(&anyhow::anyhow!(
        "invalid image"
    )));

    // A failed update must leave the existing file intact and still
    // allow a subsequent verified download.
    assert!(heif::install(&managed.library, b"tampered update").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), old_bytes);
    assert!(heif::download_would_help(&error));

    std::env::remove_var("SCHIST_LIBHEIF_DIR");
    std::fs::remove_dir_all(dir).unwrap();
}
