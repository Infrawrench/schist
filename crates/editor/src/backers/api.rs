//! Convert the backer service's flat response into the embedded tier groups.

use super::data::{default_logo_width, Backer, Backers, Logo, Tier};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Response {
    project: String,
    tiers: Vec<ApiTier>,
    backers: Vec<ApiBacker>,
}

#[derive(Deserialize)]
struct ApiTier {
    id: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiBacker {
    name: String,
    tier: String,
    website: Option<String>,
    logo_light: Option<String>,
    logo_dark: Option<String>,
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

pub fn parse(bytes: &[u8]) -> Result<Backers> {
    let response: Response =
        serde_json::from_slice(bytes).context("parsing backers API response")?;
    ensure!(response.project == "Schist", "unexpected backer project");
    let mut catalog = Backers {
        support_url: "https://backers.schist.app".into(),
        tiers: Vec::with_capacity(response.tiers.len()),
    };
    let mut tier_indices = BTreeMap::new();
    for tier in response.tiers {
        ensure!(
            !tier.id.trim().is_empty() && !tier.name.trim().is_empty(),
            "backer tier must have an id and name"
        );
        ensure!(
            tier_indices
                .insert(tier.id.clone(), catalog.tiers.len())
                .is_none(),
            "duplicate backer tier: {}",
            tier.id
        );
        catalog.tiers.push(Tier {
            name: tier.name,
            backers: Vec::new(),
        });
    }
    for backer in response.backers {
        ensure!(!backer.name.trim().is_empty(), "backer must have a name");
        let index = *tier_indices.get(&backer.tier).with_context(|| {
            format!(
                "backer {:?} has unknown tier {:?}",
                backer.name, backer.tier
            )
        })?;
        let logo = match (nonempty(backer.logo_light), nonempty(backer.logo_dark)) {
            (Some(light), dark) => Some(Logo {
                light,
                dark,
                width: default_logo_width(),
            }),
            (None, Some(light)) => Some(Logo {
                light,
                dark: None,
                width: default_logo_width(),
            }),
            (None, None) => None,
        };
        catalog.tiers[index].backers.push(Backer {
            name: backer.name,
            url: nonempty(backer.website),
            logo,
        });
    }
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_response_preserves_tier_order_and_backer_details() {
        let catalog = parse(include_bytes!("fixtures/api.json")).unwrap();
        let names: Vec<_> = catalog
            .tiers
            .iter()
            .map(|tier| tier.name.as_str())
            .collect();
        assert_eq!(names, ["Platinum", "Gold", "Silver", "Bronze"]);
        assert!(catalog.tiers[0].backers.is_empty());
        let backer = &catalog.tiers[1].backers[0];
        assert_eq!(backer.name, "LeanerCloud");
        assert_eq!(backer.url.as_deref(), Some("https://leanercloud.com"));
        let logo = backer.logo.as_ref().unwrap();
        assert_eq!(
            logo.light,
            "https://backers.schist.app/api/logos/1070272e-84cb-4a8d-8b8d-3976fbf81d88/light"
        );
        assert_eq!(
            logo.dark.as_deref(),
            Some("https://backers.schist.app/api/logos/1070272e-84cb-4a8d-8b8d-3976fbf81d88/dark")
        );
        assert_eq!(logo.width, 120);
    }

    #[test]
    fn groups_backers_and_handles_optional_details() {
        let catalog = parse(br#"{
            "project": "Schist",
            "tiers": [{"id": "gold", "name": "Gold"}, {"id": "community", "name": "Community"}],
            "backers": [
                {"name": "Person", "tier": "community", "website": "", "logoLight": null, "logoDark": " "},
                {"name": "Light", "tier": "gold", "logoLight": "https://example.com/light.png"},
                {"name": "Dark", "tier": "gold", "logoDark": "https://example.com/dark.png"}
            ]
        }"#).unwrap();
        let gold = &catalog.tiers[0].backers;
        assert_eq!(
            gold.iter()
                .map(|backer| backer.name.as_str())
                .collect::<Vec<_>>(),
            ["Light", "Dark"]
        );
        assert!(gold[0].logo.as_ref().unwrap().dark.is_none());
        assert_eq!(
            gold[1].logo.as_ref().unwrap().light,
            "https://example.com/dark.png"
        );
        let person = &catalog.tiers[1].backers[0];
        assert_eq!(person.name, "Person");
        assert!(person.url.is_none());
        assert!(person.logo.is_none());
        // Generated JSON must be readable by the runtime catalog loader.
        let encoded = serde_json::to_vec(&catalog).unwrap();
        let decoded: Backers = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.tiers[1].backers[0].name, "Person");
    }

    #[test]
    fn rejects_unknown_and_duplicate_tiers_instead_of_losing_backers() {
        assert!(parse(
            br#"{
            "project": "Schist", "tiers": [],
            "backers": [{"name": "Someone", "tier": "missing"}]
        }"#
        )
        .is_err());
        assert!(parse(
            br#"{
            "project": "Schist", "backers": [],
            "tiers": [{"id": "gold", "name": "Gold"}, {"id": "gold", "name": "Duplicate"}]
        }"#
        )
        .is_err());
        assert!(parse(br#"{"project": "Other", "tiers": [], "backers": []}"#).is_err());
    }
}
