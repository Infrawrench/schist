//! The public backers.json format, shared by the build script and the app.

use serde::Deserialize;

#[derive(Deserialize)]
pub struct Backers {
    pub support_url: String,
    pub tiers: Vec<Tier>,
}

#[derive(Deserialize)]
pub struct Tier {
    pub name: String,
    pub backers: Vec<Backer>,
}

#[derive(Deserialize)]
pub struct Backer {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub logo: Option<Logo>,
}

#[derive(Deserialize)]
pub struct Logo {
    pub light: String,
    #[serde(default)]
    pub dark: Option<String>,
    #[serde(default = "default_logo_width")]
    pub width: u32,
}

fn default_logo_width() -> u32 {
    120
}
