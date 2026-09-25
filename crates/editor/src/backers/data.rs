//! The generated backers.json format, shared by the build script and the app.

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct Backers {
    pub support_url: String,
    pub tiers: Vec<Tier>,
}

#[derive(Deserialize, Serialize)]
pub struct Tier {
    pub name: String,
    pub backers: Vec<Backer>,
}

#[derive(Deserialize, Serialize)]
pub struct Backer {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<Logo>,
}

#[derive(Deserialize, Serialize)]
pub struct Logo {
    pub light: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
    #[serde(default = "default_logo_width")]
    pub width: u32,
}

pub fn default_logo_width() -> u32 {
    120
}
