use anyhow::{bail, ensure, Result};
pub use rmpv::Value;
use serde::{Deserialize, Serialize};

pub const DEFAULT_DOMAIN: &str = "schist.app";
pub const MAX_FRAME: usize = 256 * 1024 * 1024;
/// The provider's per-file upload limit, separate from the wire frame limit.
pub const MAX_SINGLE_UPLOAD_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_UPLOAD_BYTES: u64 = 5 * 1024 * 1024 * 1024;
pub const IMAGE_MODEL: &str = "schist.image.v1";

pub fn validate_upload_size(size: u64) -> Result<()> {
    ensure!(size > 0, "File is empty");
    ensure!(
        size <= MAX_UPLOAD_BYTES,
        "Exceeds the 5 GiB per-file upload limit"
    );
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub document_models: Vec<String>,
    pub formats: Vec<Format>,
    pub max_frame_bytes: u64,
    pub max_document_bytes: u64,
    pub default_edited_export: String,
    pub original_download: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Format {
    pub id: String,
    pub name: String,
    pub extensions: Vec<String>,
    pub can_export: bool,
    pub runtime_requirement: Option<String>,
}
impl Capabilities {
    pub fn from_reply(reply: std::result::Result<Value, String>) -> Result<Option<Self>> {
        match reply {
            Ok(value) => Ok(Some(parse(value)?)),
            Err(error) if error.starts_with("method_not_found:") => Ok(None),
            Err(error) => bail!(error),
        }
    }
    pub fn frame_limit(&self) -> usize {
        self.max_frame_bytes.min(MAX_FRAME as u64) as usize
    }
    pub fn supports_image_model(&self) -> bool {
        self.document_models
            .iter()
            .any(|model| model == IMAGE_MODEL)
    }
    pub fn supports_export(&self, extension: &str) -> bool {
        self.formats
            .iter()
            .any(|format| format.can_export && format.extensions.iter().any(|ext| ext == extension))
    }
    pub fn check_document(&self, bytes: usize) -> Result<()> {
        ensure!(
            bytes as u64 <= self.max_document_bytes,
            "Shared document exceeds the provider's {} byte limit; edits remain local",
            self.max_document_bytes
        );
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: f64,
    pub generation_endpoint_url: String,
    pub logout_url: String,
    pub workspace_websocket_url: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub domain: String,
    pub exchange_url: String,
    pub credentials: Credentials,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Folder {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub revision: u64,
    /// Assets in this folder and its descendants, supplied with the folder list.
    /// Older providers may omit the count; unknown is distinct from empty.
    #[serde(default)]
    pub asset_count: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bucket {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub rule: Option<Rule>,
    /// Total matching assets, supplied with the bucket list before it is opened.
    /// Older providers may omit the count; unknown is distinct from empty.
    #[serde(default)]
    pub asset_count: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Rule {
    pub scope: Scope,
    pub text: String,
    pub filters: Filters,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Asset {
    #[serde(default)]
    pub faces: Vec<Face>,
    #[serde(default)]
    pub moderation: Option<Moderation>,
    pub id: String,
    pub folder_id: Option<String>,
    pub name: String,
    pub mime_type: String,
    pub revision: u64,
    pub size: u64,
    pub edited: bool,
    pub tags: Vec<String>,
    pub rating: u8,
    pub captured_at: Option<u64>,
    pub modified_at: u64,
    pub thumbnail_url: Option<String>,
    /// Nearest city from EXIF, absent on older providers or photos without a location.
    #[serde(default)]
    pub place_name: Option<String>,
    /// The EXIF fix itself, for the world map; absent on older providers.
    #[serde(default)]
    pub location: Option<Location>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Library,
    Folder {
        id: String,
        recursive: bool,
    },
    Bucket {
        id: String,
    },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Filters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_types: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_after: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_before: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_rating: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bounds {
    pub south: f64,
    pub north: f64,
    pub west: f64,
    pub east: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AssetQuery {
    pub scope: Scope,
    pub text: String,
    pub filters: Filters,
    pub sort: String,
    pub offset: u64,
    pub limit: u64,
}
impl Default for AssetQuery {
    fn default() -> Self {
        Self {
            scope: Scope::Library,
            text: String::new(),
            filters: Filters::default(),
            sort: "name".into(),
            offset: 0,
            limit: 100,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogueQuery {
    pub text: String,
    pub offset: u64,
    pub limit: u64,
}
impl Default for CatalogueQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            offset: 0,
            limit: 500,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WatchQuery {
    Folders { query: CatalogueQuery },
    Buckets { query: CatalogueQuery },
    Assets { query: Box<AssetQuery> },
}
#[derive(Clone, Debug, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub screening: Option<Screening>,
    #[serde(default)]
    pub people: Option<People>,
    pub kind: String,
    pub revision: u64,
    pub total: u64,
    /// Whole-library asset total, independent of this query's filters and page.
    #[serde(default)]
    pub library_asset_count: Option<u64>,
    pub offset: u64,
    pub items: Vec<Value>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Failure {
    pub code: String,
    pub message: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Ready {
        protocol: u32,
    },
    Result {
        id: String,
        value: Value,
    },
    Error {
        id: String,
        error: Failure,
    },
    Snapshot {
        subscription_id: String,
        snapshot: Snapshot,
    },
    WatchError {
        subscription_id: String,
        error: Failure,
    },
    DocumentUpdate {
        document_id: String,
        #[serde(with = "serde_bytes")]
        update: Vec<u8>,
    },
    DocumentError {
        document_id: String,
        error: Failure,
    },
    AuthExpiring,
    Pong,
}

pub fn value<T: Serialize>(v: T) -> Value {
    rmp_serde::from_slice(&rmp_serde::to_vec_named(&v).expect("serializable protocol value"))
        .expect("valid MessagePack value")
}
pub fn parse<T: for<'de> Deserialize<'de>>(v: Value) -> Result<T> {
    Ok(rmpv::ext::from_value(v)?)
}
pub fn map(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Map(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
pub fn field<'a>(v: &'a Value, name: &str) -> Result<&'a Value> {
    v.as_map()
        .and_then(|m| {
            m.iter()
                .find(|(k, _)| k.as_str() == Some(name))
                .map(|(_, v)| v)
        })
        .ok_or_else(|| anyhow::anyhow!("missing {name}"))
}
pub fn bytes(v: &Value, name: &str) -> Result<Vec<u8>> {
    match field(v, name)? {
        Value::Binary(b) => Ok(b.clone()),
        _ => bail!("{name} must be MessagePack binary"),
    }
}
pub fn string(v: &Value, name: &str) -> Result<String> {
    Ok(field(v, name)?
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("{name} must be a string"))?
        .into())
}
pub fn encode(v: &Value) -> Result<Vec<u8>> {
    encode_with_limit(v, MAX_FRAME)
}
pub fn encode_with_limit(v: &Value, limit: usize) -> Result<Vec<u8>> {
    let b = rmp_serde::to_vec_named(v)?;
    ensure!(
        b.len() <= limit.min(MAX_FRAME),
        "Message exceeds the {} byte workspace frame limit",
        limit.min(MAX_FRAME)
    );
    Ok(b)
}
pub fn decode(b: &[u8]) -> Result<ServerMessage> {
    ensure!(b.len() <= MAX_FRAME, "Message exceeds 256 MiB");
    let mut cursor = std::io::Cursor::new(b);
    let v = rmpv::decode::read_value_with_max_depth(&mut cursor, 64)?;
    ensure!(
        cursor.position() as usize == b.len(),
        "Trailing MessagePack data"
    );
    // serde_bytes also accepts arrays, but the wire contract specifically requires bin.
    if field(&v, "type")?.as_str() == Some("document_update") {
        bytes(&v, "update")?;
    }
    parse(v)
}
pub fn parse_date(raw: &str, end: bool) -> Result<Option<u64>> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let date = chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")?;
    let time = date
        .and_hms_opt(
            if end { 23 } else { 0 },
            if end { 59 } else { 0 },
            if end { 59 } else { 0 },
        )
        .unwrap()
        .and_utc()
        .timestamp();
    ensure!(time >= 0, "Date precedes 1970");
    Ok(Some(time as u64))
}
pub fn format_date(t: u64) -> String {
    chrono::DateTime::from_timestamp(t as i64, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn empty_catalogues_carry_library_totals_with_legacy_support() {
        for count in [None, Some(0_u64), Some(5000)] {
            let mut snapshot =
                serde_json::json!({"kind":"folders","revision":1,"total":0,"offset":0,"items":[]});
            if let Some(count) = count {
                snapshot["library_asset_count"] = count.into();
            }
            let decoded: Snapshot =
                rmp_serde::from_slice(&rmp_serde::to_vec_named(&snapshot).unwrap()).unwrap();
            assert_eq!(decoded.library_asset_count, count);
            assert_eq!(decoded.total, 0);
        }
    }
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn asset_place_names_are_optional_for_older_providers() {
        let mut asset = serde_json::json!({"id":"a","folder_id":null,"name":"photo.jpg",
            "mime_type":"image/jpeg","revision":1,"size":1,"edited":false,
            "tags":[],"rating":0,"captured_at":null,"modified_at":1,"thumbnail_url":null});
        for name in [None, Some("New York City")] {
            if let Some(name) = name {
                asset["place_name"] = name.into();
            }
            let decoded: Asset =
                rmp_serde::from_slice(&rmp_serde::to_vec_named(&asset).unwrap()).unwrap();
            assert_eq!(decoded.place_name.as_deref(), name);
        }
    }
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn folder_counts_arrive_with_the_catalogue() {
        for count in [None, Some(0_u64), Some(1234)] {
            let mut folder =
                serde_json::json!({"id":"f","name":"Photos","revision":1,"parent_id":null});
            if let Some(count) = count {
                folder["asset_count"] = count.into();
            }
            let bytes = rmp_serde::to_vec_named(&folder).unwrap();
            let decoded: Folder = rmp_serde::from_slice(&bytes).unwrap();
            assert_eq!(decoded.asset_count, count);
        }
    }
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn bucket_counts_arrive_with_the_catalogue() {
        for count in [None, Some(0_u64), Some(1234)] {
            let mut bucket = serde_json::json!({"id":"b","name":"Summer","revision":1,"rule":null});
            if let Some(count) = count {
                bucket["asset_count"] = count.into();
            }
            let bytes = rmp_serde::to_vec_named(&bucket).unwrap();
            let decoded: Bucket = rmp_serde::from_slice(&bytes).unwrap();
            assert_eq!(decoded.asset_count, count);
        }
    }
    pub(super) fn capabilities() -> Capabilities {
        Capabilities {
            document_models: vec![IMAGE_MODEL.into()],
            formats: vec![Format {
                id: "codec.png".into(),
                name: "PNG".into(),
                extensions: vec!["png".into()],
                can_export: true,
                runtime_requirement: None,
            }],
            max_frame_bytes: MAX_FRAME as u64,
            max_document_bytes: 128 * 1024 * 1024,
            default_edited_export: "psd".into(),
            original_download: true,
        }
    }
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn capabilities_fallback_only_for_unknown_method() {
        assert!(
            Capabilities::from_reply(Err("method_not_found: older provider".into()))
                .unwrap()
                .is_none()
        );
        assert!(Capabilities::from_reply(Err("forbidden: permission denied".into())).is_err());
        assert!(Capabilities::from_reply(Err("Cloud disconnected".into())).is_err());
        let caps = Capabilities::from_reply(Ok(value(capabilities())))
            .unwrap()
            .unwrap();
        assert!(caps.supports_image_model());
        assert!(caps.supports_export("png"));
        assert!(!caps.supports_export("heic"));
    }
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn document_limit_is_independent_of_frame_limit() {
        let caps = capabilities();
        assert!(caps.check_document(128 * 1024 * 1024).is_ok());
        assert!(caps.check_document(128 * 1024 * 1024 + 1).is_err());
        let message = map([("update", Value::Binary(vec![0; 24]))]);
        let size = encode(&message).unwrap().len();
        assert!(size > 24);
        assert!(encode_with_limit(&message, size).is_ok());
        assert!(encode_with_limit(&message, size - 1).is_err());
        let mut caps = caps;
        caps.max_frame_bytes = u64::MAX;
        assert_eq!(caps.frame_limit(), MAX_FRAME);
    }
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn binary_round_trip_and_trailing_data() {
        let b: Vec<u8> = (0..=255).collect();
        let v = map([
            ("type", "document_update".into()),
            ("document_id", "asset".into()),
            ("update", Value::Binary(b.clone())),
        ]);
        let encoded = encode(&v).unwrap();
        match decode(&encoded).unwrap() {
            ServerMessage::DocumentUpdate { update, .. } => assert_eq!(update, b),
            _ => panic!(),
        }
        let mut trailing = encoded;
        trailing.push(0);
        assert!(decode(&trailing).is_err());
        let array = map([
            ("type", "document_update".into()),
            ("document_id", "asset".into()),
            ("update", Value::Array(vec![1.into(), 2.into()])),
        ]);
        assert!(decode(&encode(&array).unwrap()).is_err());
    }
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn absent_filters_are_not_nil() {
        let v = value(Filters::default());
        assert!(v.as_map().unwrap().is_empty());
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Screening {
    pub pending: u64,
    #[serde(default)]
    pub blocked: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Moderation {
    pub status: String,
    pub revision: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct People {
    pub enabled: bool,
    pub pending: u64,
    #[serde(default)]
    pub failed: u64,
    pub unnamed: u64,
    pub people: Vec<Person>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub asset_count: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FaceRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Face {
    pub id: String,
    pub rect: FaceRect,
    #[serde(default)]
    pub person_id: Option<String>,
    #[serde(default)]
    pub suggestion: Option<String>,
    #[serde(default)]
    pub automatic: bool,
}

#[cfg(test)]
mod people_contract_tests {
    use super::*;
    #[test]
    fn upload_size_checks_match_the_provider_boundary() {
        assert!(validate_upload_size(1).is_ok());
        assert!(validate_upload_size(MAX_UPLOAD_BYTES).is_ok());
        assert!(validate_upload_size(0)
            .unwrap_err()
            .to_string()
            .contains("empty"));
        assert!(validate_upload_size(MAX_UPLOAD_BYTES + 1)
            .unwrap_err()
            .to_string()
            .contains("5 GiB"));
        assert!(validate_upload_size(u64::MAX).is_err());
    }
    #[test]
    fn screening_and_people_are_optional_for_legacy_providers() {
        let old: Snapshot = serde_json::from_str(
            r#"{"kind":"assets","revision":0,"total":0,"offset":0,"items":[]}"#,
        )
        .unwrap();
        assert!(old.screening.is_none());
        assert!(old.people.is_none());
        let current: Snapshot=serde_json::from_str(r#"{"kind":"assets","revision":1,"total":0,"offset":0,"items":[],"screening":{"pending":2,"blocked":1},"people":{"enabled":true,"pending":1,"unnamed":2,"people":[{"id":"p","name":"Ann","asset_count":3}]}}"#).unwrap();
        assert_eq!(current.screening.unwrap().pending, 2);
        assert_eq!(current.people.unwrap().people[0].asset_count, 3);
    }
}
