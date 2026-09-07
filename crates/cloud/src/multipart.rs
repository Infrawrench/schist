//! Bounded-memory uploads. A content/destination key finds the same durable
//! server ticket when a user selects an interrupted file again (24-hour expiry).
use crate::{auth, protocol::*, runtime, Handle};
use anyhow::{ensure, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;

pub const PART_BYTES: usize = 8 * 1024 * 1024;
pub struct ChunkUpload<'a> {
    pub name: &'a str,
    pub size: u64,
    pub mime: &'a str,
    pub folder: Option<&'a str>,
    pub relative: Option<&'a str>,
    pub resume_key: String,
}
#[derive(Deserialize)]
struct Status {
    #[serde(default)]
    asset: Option<Asset>,
    #[serde(default)]
    received: bool,
    #[serde(default)]
    parts: Vec<Part>,
}
#[derive(Deserialize)]
struct Part {
    number: u64,
    size: u64,
}
fn key(
    digest: &[u8],
    name: &str,
    mime: &str,
    folder: Option<&str>,
    relative: Option<&str>,
) -> String {
    let mut hash = Sha256::new();
    hash.update(digest);
    hash.update(serde_json::to_vec(&(name, mime, folder, relative)).expect("string tuple"));
    format!("{:x}", hash.finalize())
}
impl Handle {
    pub async fn upload_chunks_async(
        &self,
        upload: ChunkUpload<'_>,
        mut read: impl FnMut(u64, usize) -> Result<Vec<u8>>,
        mut progress: impl FnMut(u64),
    ) -> Result<Asset> {
        validate_upload_size(upload.size)?;
        let mut fields = vec![
            ("name", upload.name.into()),
            ("mime_type", upload.mime.into()),
            ("size", upload.size.into()),
            ("resume_key", upload.resume_key.into()),
            ("mutation_id", uuid::Uuid::new_v4().to_string().into()),
        ];
        if let Some(folder) = upload.folder {
            fields.push(("folder_id", folder.into()));
        }
        if let Some(relative) = upload.relative {
            fields.push(("relative_path", relative.into()));
        }
        let ticket = self
            .request_async("asset.prepare_multipart", map(fields))
            .await?;
        let id = string(&ticket, "upload_id")?;
        let part_size = field(&ticket, "part_size")?.as_u64().unwrap_or(0);
        ensure!(
            part_size == PART_BYTES as u64,
            "Unsupported upload chunk size"
        );
        let status: Status = parse(
            self.request_async(
                "asset.multipart_status",
                map([
                    ("upload_id", id.clone().into()),
                    ("mutation_id", uuid::Uuid::new_v4().to_string().into()),
                ]),
            )
            .await?,
        )?;
        if let Some(asset) = status.asset {
            progress(upload.size);
            return Ok(asset);
        }
        let mut done = 0;
        if !status.received {
            for (index, offset) in (0..upload.size).step_by(PART_BYTES).enumerate() {
                let number = index as u64 + 1;
                let length = (upload.size - offset).min(part_size) as usize;
                if !status
                    .parts
                    .iter()
                    .any(|p| p.number == number && p.size == length as u64)
                {
                    let bytes = read(offset, length)?;
                    ensure!(
                        bytes.len() == length,
                        "The source file changed during upload"
                    );
                    for attempt in 0..3 {
                        let result = async {
                            let part = self
                                .request_async(
                                    "asset.multipart_part",
                                    map([
                                        ("upload_id", id.clone().into()),
                                        ("part_number", number.into()),
                                        ("mutation_id", uuid::Uuid::new_v4().to_string().into()),
                                    ]),
                                )
                                .await?;
                            auth::upload_async(
                                &string(&part, "put_url")?,
                                "application/octet-stream",
                                &bytes,
                            )
                            .await
                        }
                        .await;
                        match result {
                            Ok(()) => break,
                            Err(error) if attempt == 2 => {
                                return Err(error.context(
                                    "Select this file again to resume its uploaded parts",
                                ))
                            }
                            Err(_) => runtime::sleep(Duration::from_secs(1 << attempt)).await,
                        }
                    }
                }
                done += length as u64;
                progress(done);
            }
        }
        parse(
            self.request_async(
                "asset.commit_upload",
                map([
                    ("upload_id", id.into()),
                    ("mutation_id", uuid::Uuid::new_v4().to_string().into()),
                ]),
            )
            .await?,
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn upload_path_async(
        &self,
        path: &std::path::Path,
        mime: &str,
        folder: Option<&str>,
        relative: Option<&str>,
        progress: impl FnMut(u64),
    ) -> Result<Asset> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        validate_upload_size(metadata.len())?;
        let modified = metadata.modified()?;
        let digest = file_digest(&mut file, &metadata)?;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let resume_key = key(&digest, &name, mime, folder, relative);
        self.upload_chunks_async(
            ChunkUpload {
                name: &name,
                size: metadata.len(),
                mime,
                folder,
                relative,
                resume_key,
            },
            |offset, length| {
                let current = file.metadata()?;
                ensure!(
                    current.len() == metadata.len() && current.modified()? == modified,
                    "The source file changed; select it again to start a new upload"
                );
                file.seek(SeekFrom::Start(offset))?;
                let mut bytes = vec![0; length];
                file.read_exact(&mut bytes)?;
                Ok(bytes)
            },
            progress,
        )
        .await
    }
    pub async fn upload_large_bytes_async(&self, upload: crate::Upload<'_>) -> Result<Asset> {
        ensure!(
            upload.asset.is_none(),
            "Large collaborative replacements use the document API"
        );
        let resume_key = key(
            &Sha256::digest(upload.bytes),
            upload.name,
            upload.mime,
            upload.folder,
            upload.relative,
        );
        self.upload_chunks_async(
            ChunkUpload {
                name: upload.name,
                size: upload.bytes.len() as u64,
                mime: upload.mime,
                folder: upload.folder,
                relative: upload.relative,
                resume_key,
            },
            |offset, length| Ok(upload.bytes[offset as usize..offset as usize + length].to_vec()),
            |_| {},
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resume_identity_includes_contents_and_destination() {
        let digest = Sha256::digest(b"original");
        let original = key(
            &digest,
            "photo.tiff",
            "image/tiff",
            Some("folder"),
            Some("trip/photo.tiff"),
        );
        assert_eq!(
            original,
            key(
                &digest,
                "photo.tiff",
                "image/tiff",
                Some("folder"),
                Some("trip/photo.tiff")
            )
        );
        assert_ne!(
            original,
            key(
                &Sha256::digest(b"changed"),
                "photo.tiff",
                "image/tiff",
                Some("folder"),
                Some("trip/photo.tiff")
            )
        );
        assert_ne!(
            original,
            key(
                &digest,
                "photo.tiff",
                "image/tiff",
                Some("other"),
                Some("trip/photo.tiff")
            )
        );
        assert_ne!(
            original,
            key(
                &digest,
                "photo.tiff",
                "image/tiff",
                Some("folder"),
                Some("other/photo.tiff")
            )
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn file_digest(file: &mut std::fs::File, metadata: &std::fs::Metadata) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; PART_BYTES];
    let mut hashed = 0u64;
    loop {
        let length = file.read(&mut buffer)?;
        hashed += length as u64;
        ensure!(
            hashed <= metadata.len(),
            "The source file grew during upload preparation"
        );
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    ensure!(
        hashed == metadata.len() && file.metadata()?.modified()? == metadata.modified()?,
        "The source file changed during upload preparation"
    );
    drop(buffer);
    Ok(digest.finalize().to_vec())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn resume_key_for_path(
    path: &std::path::Path,
    mime: &str,
    folder: Option<&str>,
    relative: Option<&str>,
) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    validate_upload_size(metadata.len())?;
    let digest = file_digest(&mut file, &metadata)?;
    Ok(key(
        &digest,
        &path.file_name().unwrap_or_default().to_string_lossy(),
        mime,
        folder,
        relative,
    ))
}
pub fn resume_key_for_bytes(
    bytes: &[u8],
    name: &str,
    mime: &str,
    folder: Option<&str>,
    relative: Option<&str>,
) -> String {
    key(&Sha256::digest(bytes), name, mime, folder, relative)
}
#[derive(Deserialize)]
pub struct UploadCapacity {
    pub fits: bool,
    pub additional_bytes: u64,
    pub remaining_bytes: Option<u64>,
    pub shortfall_bytes: u64,
}
impl UploadCapacity {
    pub fn require_space(&self) -> Result<()> {
        let format = |bytes: u64| {
            if bytes >= 1024 * 1024 * 1024 {
                format!("{:.2} GiB", bytes as f64 / (1024f64.powi(3)))
            } else if bytes >= 1024 * 1024 {
                format!("{:.2} MiB", bytes as f64 / (1024f64.powi(2)))
            } else {
                format!("{bytes} bytes")
            }
        };
        ensure!(self.fits,"Not enough cloud storage. This selection needs {}; {} is available. Free up {} or upgrade your plan. No files were uploaded.",format(self.additional_bytes),format(self.remaining_bytes.unwrap_or(0)),format(self.shortfall_bytes));
        Ok(())
    }
}
