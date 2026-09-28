//! Offline source conversion: make export-background-coreml-sources.
#[cfg(all(
    target_os = "macos",
    target_arch = "aarch64",
    feature = "coreml-export"
))]
fn main() -> anyhow::Result<()> {
    for id in ["detail-matting", "subject-guide", "matting"] {
        let path = schist_neural::export_coreml_source(id)?;
        println!("{id}\t{}", path.display());
    }
    Ok(())
}
#[cfg(not(all(
    target_os = "macos",
    target_arch = "aarch64",
    feature = "coreml-export"
)))]
fn main() {
    eprintln!("Use make export-background-coreml-sources on Apple Silicon.");
    std::process::exit(1);
}
