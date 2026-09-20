//! Typed catalogue extensions shared by desktop and browser clients.
use crate::{
    protocol::{map, parse, value},
    Asset, Handle,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Metadata {
    pub metadata_revision: u64,
    pub flag: String,
    pub label: String,
    pub caption: String,
    pub copyright: String,
    pub review: Option<ReviewChoice>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewChoice {
    pub revision: u64,
    pub choice: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub id: String,
    pub revision: u64,
    pub metadata_revision: u64,
}
impl From<&Asset> for Target {
    fn from(asset: &Asset) -> Self {
        Self {
            id: asset.id.clone(),
            revision: asset.revision,
            metadata_revision: asset.metadata.metadata_revision,
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct Version {
    pub id: String,
    pub revision: u64,
    pub name: String,
    pub bytes: u64,
    pub created_at: u64,
    pub download_url: String,
    pub thumbnail_url: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Signature {
    pub hash: String,
    pub rgb: Vec<u8>,
    pub aspect: f32,
}
#[derive(Clone, Debug, Deserialize)]
pub struct ReviewPhoto {
    pub asset: Asset,
    pub signature: Signature,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workflow {
    pub kind: String,
    pub revision: u64,
    pub data: serde_json::Value,
}

impl Handle {
    pub async fn versions(&self, id: &str) -> Result<Vec<Version>> {
        #[derive(Deserialize)]
        struct Reply {
            versions: Vec<Version>,
        }
        let reply: Reply = parse(
            self.request_async("asset.versions", map([("id", id.into())]))
                .await?,
        )?;
        Ok(reply.versions)
    }
    pub async fn review_photos(&self, assets: &[Asset]) -> Result<Vec<ReviewPhoto>> {
        #[derive(Deserialize)]
        struct Reply {
            photos: Vec<ReviewPhoto>,
        }
        let reply: Reply = parse(
            self.request_async(
                "assets.review",
                map([(
                    "assets",
                    value(&assets.iter().map(Target::from).collect::<Vec<_>>()),
                )]),
            )
            .await?,
        )?;
        Ok(reply.photos)
    }
    pub async fn workflows(&self) -> Result<Vec<Workflow>> {
        #[derive(Deserialize)]
        struct Reply {
            libraries: Vec<Workflow>,
        }
        let reply: Reply = parse(self.request_async("workflows.get", map([])).await?)?;
        Ok(reply.libraries)
    }
    pub async fn save_workflow(&self, workflow: &Workflow) -> Result<Workflow> {
        // Textured tips can contain millions of byte samples. Carry the bounded
        // JSON library as a string, outside the socket's generic array limit.
        parse(
            self.request_async(
                "workflows.update",
                map([
                    ("kind", workflow.kind.clone().into()),
                    ("revision", workflow.revision.into()),
                    ("data_json", serde_json::to_string(&workflow.data)?.into()),
                    ("mutation_id", crate::Uuid::new_v4().to_string().into()),
                ]),
            )
            .await?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_catalogues_default_and_new_decisions_roundtrip() {
        let old: Metadata = serde_json::from_str("{}").unwrap();
        assert_eq!(old.metadata_revision, 0);
        let current: Metadata = serde_json::from_str(r#"{"metadata_revision":3,"flag":"pick","label":"blue","caption":"Caption","review":{"revision":7,"choice":"keep"}}"#).unwrap();
        let decoded: Metadata = parse(value(&current)).unwrap();
        assert_eq!(decoded, current);
    }
}

pub fn parse_time(text: &str) -> Result<Option<u64>> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let timestamp = chrono::DateTime::parse_from_rfc3339(text.trim())
        .map_err(|_| anyhow::anyhow!(schist_i18n::t("metadata.invalid")))?
        .timestamp();
    Ok(Some(u64::try_from(timestamp).map_err(|_| {
        anyhow::anyhow!(schist_i18n::t("metadata.invalid"))
    })?))
}
pub fn format_time(timestamp: Option<u64>) -> String {
    timestamp
        .and_then(|t| i64::try_from(t).ok())
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}
