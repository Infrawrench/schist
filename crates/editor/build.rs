//! Download and embed the logos referenced by the shipped backer catalog.

#[path = "../../tools/app-cfg.rs"]
mod app_cfg;
#[allow(dead_code)]
#[path = "src/backers/data.rs"]
mod data;

use std::{collections::BTreeSet, env, fmt::Write as _, fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

fn main() -> Result<()> {
    app_cfg::main();
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src/backers/data.rs");
    println!("cargo::rerun-if-changed=../../backers.json");
    println!("cargo::rerun-if-env-changed=SCHIST_REFRESH_BACKER_LOGOS");

    let catalog: data::Backers = serde_json::from_str(&fs::read_to_string("../../backers.json")?)
        .context("parsing backers.json")?;
    let out = PathBuf::from(env::var_os("OUT_DIR").context("OUT_DIR is not set")?);
    let cache = out.join("backer-logos");
    fs::create_dir_all(&cache)?;
    let refresh = env::var("SCHIST_REFRESH_BACKER_LOGOS").as_deref() == Ok("1");
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .build(),
    );
    let urls: BTreeSet<_> = catalog
        .tiers
        .iter()
        .flat_map(|tier| &tier.backers)
        .filter_map(|backer| backer.logo.as_ref())
        .flat_map(|logo| std::iter::once(&logo.light).chain(logo.dark.iter()))
        .collect();
    let mut generated = String::from("const LOGOS: &[(&str, &[u8])] = &[\n");
    for url in urls {
        anyhow::ensure!(
            url.starts_with("https://") || url.starts_with("http://"),
            "backer logo URL must use HTTP(S): {url}"
        );
        let path = cache.join(format!("{:x}.img", Sha256::digest(url.as_bytes())));
        if refresh || !path.is_file() {
            let bytes = agent
                .get(url)
                .call()
                .with_context(|| format!("downloading backer logo {url}"))?
                .body_mut()
                .with_config()
                .limit(8 << 20)
                .read_to_vec()
                .with_context(|| format!("reading backer logo {url}"))?;
            anyhow::ensure!(!bytes.is_empty(), "empty backer logo: {url}");
            let temporary = path.with_extension("tmp");
            fs::write(&temporary, bytes)?;
            fs::rename(&temporary, &path)?;
        }
        writeln!(generated, "    ({url:?}, include_bytes!({path:?})),")?;
    }
    generated.push_str("];\n");
    fs::write(out.join("backer_logos.rs"), generated)?;
    Ok(())
}
