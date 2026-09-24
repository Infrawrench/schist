fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // Give this crate's test executables the same privacy declarations as
        // the app. Library consumers supply their own application metadata.
        let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let plist = manifest.join("../../packaging/macos/Info.plist");
        println!("cargo::rerun-if-changed={}", plist.display());
        println!(
            "cargo::rustc-link-arg=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
}
