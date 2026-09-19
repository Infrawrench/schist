//! Bounded embedded smart-object documents. Kept in a private layer block so
//! history, PSD and shared-document snapshots carry the editable source together.
use crate::{Layer, RawBlock};
use anyhow::{ensure, Context, Result};
use schist_i18n::t;
use serde::{Deserialize, Serialize};

pub const SOURCE_DOCUMENT_KEY: [u8; 4] = *b"ScSd";
pub const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SOURCE_PIXELS: u64 = 32_000_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceIdentity {
    pub version: u32,
    pub id: String,
    /// Native absolute path. Never read automatically when opening a document.
    pub linked_path: Option<String>,
    /// Nested document origin in the original smart source coordinates.
    #[serde(default)]
    pub origin: [i32; 2],
    /// Size and modification timestamp when explicitly loaded.
    #[serde(default)]
    pub linked_stamp: Option<[u64; 2]>,
}

/// A nested PSD, decoded only when the user chooses Edit Contents.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartSource {
    pub identity: SourceIdentity,
    pub document: Vec<u8>,
}

impl SmartSource {
    fn parts(layer: &Layer) -> Result<Option<(SourceIdentity, &[u8])>> {
        let Some(block) = layer.extras.iter().find(|b| b.key == SOURCE_DOCUMENT_KEY) else {
            return Ok(None);
        };
        let data = &block.data;
        ensure!(
            data.len() <= MAX_DOCUMENT_BYTES + 16_388,
            "{}",
            t("smart.error.too_large")
        );
        let header: [u8; 4] = data
            .get(..4)
            .context(t("smart.error.invalid"))?
            .try_into()?;
        let n = u32::from_be_bytes(header) as usize;
        ensure!(n <= 16_384, "{}", t("smart.error.invalid"));
        let identity: SourceIdentity =
            serde_json::from_slice(data.get(4..4 + n).context(t("smart.error.invalid"))?)?;
        let document = data.get(4 + n..).context(t("smart.error.invalid"))?;
        ensure!(
            identity.version == 1
                && !identity.id.is_empty()
                && identity.id.len() <= 256
                && identity
                    .linked_path
                    .as_ref()
                    .is_none_or(|p| p.len() <= 8192)
                && identity.origin.iter().all(|v| v.abs_diff(0) <= 1_000_000)
                && document.len() <= MAX_DOCUMENT_BYTES
                && document.starts_with(b"8BPS"),
            "{}",
            t("smart.error.invalid")
        );
        Ok(Some((identity, document)))
    }

    /// Inspect the source without copying its nested document.
    pub fn identity(layer: &Layer) -> Result<Option<SourceIdentity>> {
        Ok(Self::parts(layer)?.map(|(identity, _)| identity))
    }

    pub fn read(layer: &Layer) -> Result<Option<Self>> {
        Ok(Self::parts(layer)?.map(|(identity, document)| Self {
            identity,
            document: document.to_vec(),
        }))
    }

    pub fn blocks(&self, layer: &Layer) -> Result<Vec<RawBlock>> {
        ensure!(
            self.document.len() <= MAX_DOCUMENT_BYTES,
            "{}",
            t("smart.error.too_large")
        );
        let json = serde_json::to_vec(&self.identity)?;
        ensure!(json.len() <= 16_384, "{}", t("smart.error.invalid"));
        let mut data = Vec::with_capacity(4 + json.len() + self.document.len());
        data.extend_from_slice(&(json.len() as u32).to_be_bytes());
        data.extend(json);
        data.extend_from_slice(&self.document);
        let mut extras: Vec<_> = layer
            .extras
            .iter()
            .filter(|b| b.key != SOURCE_DOCUMENT_KEY)
            .cloned()
            .collect();
        extras.push(RawBlock {
            key: SOURCE_DOCUMENT_KEY,
            data,
        });
        Ok(extras)
    }
}
