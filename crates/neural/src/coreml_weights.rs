//! Lossless external storage for ORT's large inline Core ML constants.
//! Public formats: apple/coremltools mlmodel/format/{Model,MIL}.proto and
//! mlmodel/src/MILBlob/Blob/{StorageFormat,BlobDataType}.hpp (BSD-3-Clause).
//! Preserve unknown protobuf fields; only replace float32 constant payloads.
use anyhow::{ensure, Context, Result};
use std::path::{Path, PathBuf};

const LIMIT: usize = 512 * 1024 * 1024;
struct Field {
    key: u64,
    data: Vec<u8>,
}
fn varint(input: &mut &[u8]) -> Result<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let (&byte, rest) = input.split_first().context("truncated protobuf varint")?;
        *input = rest;
        ensure!(shift < 63 || byte <= 1, "protobuf varint overflow");
        value |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return Ok(value);
        }
    }
    anyhow::bail!("invalid protobuf varint")
}
fn put(mut value: u64, output: &mut Vec<u8>) {
    while value >= 128 {
        output.push(value as u8 | 128);
        value >>= 7;
    }
    output.push(value as u8);
}
fn fields(mut input: &[u8]) -> Result<Vec<Field>> {
    ensure!(input.len() <= LIMIT, "oversized Core ML message");
    let mut result = Vec::new();
    while !input.is_empty() {
        ensure!(result.len() < 1_000_000, "too many Core ML fields");
        let key = varint(&mut input)?;
        ensure!(
            key >> 3 > 0 && key >> 3 <= 0x1fff_ffff,
            "invalid protobuf field"
        );
        let size = match key & 7 {
            0 => {
                let before = input.len();
                let mut remaining = input;
                varint(&mut remaining)?;
                before - remaining.len()
            }
            1 => 8,
            2 => usize::try_from(varint(&mut input)?)?,
            5 => 4,
            _ => anyhow::bail!("unsupported protobuf wire type"),
        };
        ensure!(size <= input.len(), "truncated Core ML field");
        result.push(Field {
            key,
            data: input[..size].to_vec(),
        });
        input = &input[size..];
    }
    Ok(result)
}
fn encode(fields: &[Field]) -> Vec<u8> {
    let mut output = Vec::new();
    for field in fields {
        put(field.key, &mut output);
        if field.key & 7 == 2 {
            put(field.data.len() as u64, &mut output);
        }
        output.extend_from_slice(&field.data);
    }
    output
}
fn bytes(number: u64, data: Vec<u8>) -> Field {
    Field {
        key: number * 8 + 2,
        data,
    }
}
fn integer(number: u64, value: u64) -> Field {
    let mut data = Vec::new();
    put(value, &mut data);
    Field {
        key: number * 8,
        data,
    }
}
fn one(fields: &[Field], key: u64) -> Result<&[u8]> {
    let mut matches = fields.iter().filter(|f| f.key == key);
    let result = matches.next().context("missing Core ML field")?;
    ensure!(matches.next().is_none(), "duplicate Core ML field");
    Ok(&result.data)
}

struct Blobs {
    data: Vec<u8>,
    count: u32,
}
impl Blobs {
    fn new() -> Self {
        let mut data = vec![0; 64];
        data[4..8].copy_from_slice(&2u32.to_le_bytes());
        Self { data, count: 0 }
    }
    fn write(&mut self, floats: &[u8]) -> Result<u64> {
        ensure!(floats.len().is_multiple_of(4), "invalid float32 bytes");
        let offset = self.data.len().next_multiple_of(64);
        ensure!(
            offset
                .checked_add(64)
                .and_then(|n| n.checked_add(floats.len()))
                .is_some_and(|n| n <= LIMIT),
            "Core ML weights exceed bound"
        );
        self.data.resize(offset + 64, 0);
        let metadata = &mut self.data[offset..];
        metadata[0..4].copy_from_slice(&0xdead_beefu32.to_le_bytes());
        metadata[4..8].copy_from_slice(&2u32.to_le_bytes()); // Float32
        metadata[8..16].copy_from_slice(&(floats.len() as u64).to_le_bytes());
        metadata[16..24].copy_from_slice(&((offset + 64) as u64).to_le_bytes());
        self.data.extend_from_slice(floats);
        self.count += 1;
        self.data[..4].copy_from_slice(&self.count.to_le_bytes());
        Ok(offset as u64)
    }
}

fn constant(input: &[u8], blobs: &mut Blobs) -> Result<Vec<u8>> {
    let mut value = fields(input)?;
    if !value.iter().any(|f| f.key == 26) {
        return Ok(input.to_vec());
    }
    let immediate = fields(one(&value, 26)?)?;
    if !immediate.iter().any(|f| f.key == 10) {
        return Ok(input.to_vec());
    }
    let tensor = fields(one(&immediate, 10)?)?;
    if !tensor.iter().any(|f| f.key == 10) {
        return Ok(input.to_vec());
    }
    let floats = fields(one(&tensor, 10)?)?;
    let mut data = Vec::new();
    for field in floats {
        ensure!(
            matches!(field.key, 10 | 13),
            "unsupported float constant encoding"
        );
        data.extend_from_slice(&field.data);
    }
    ensure!(
        data.len().is_multiple_of(4),
        "invalid float constant length"
    );
    if data.len() < 40 {
        return Ok(input.to_vec());
    }
    let ty = fields(one(&value, 18)?)?;
    let tensor_type = fields(one(&ty, 10)?)?;
    ensure!(
        varint(&mut one(&tensor_type, 8)?)? == 11,
        "constant is not float32"
    );
    let mut count = 1usize;
    for dimension in tensor_type.iter().filter(|f| f.key == 26) {
        let dimension = fields(&dimension.data)?;
        let fixed = fields(one(&dimension, 10)?)?;
        let size = usize::try_from(varint(&mut one(&fixed, 8)?)?)?;
        count = count
            .checked_mul(size)
            .context("constant dimensions overflow")?;
    }
    ensure!(
        count.checked_mul(4) == Some(data.len()),
        "constant shape mismatch"
    );
    let offset = blobs.write(&data)?;
    value.retain(|f| !matches!(f.key, 26 | 42));
    value.push(bytes(
        5,
        encode(&[
            bytes(1, b"@model_path/weights/schist-inline.bin".to_vec()),
            integer(2, offset),
        ]),
    ));
    Ok(encode(&value))
}

// Model.mlProgram -> functions.value -> block_specializations.value ->
// operations -> attributes.value. Only the const operation's `val` attribute
// may be replaced. No operator, shape, precision or feature name is changed.
fn rewrite(input: &[u8], depth: usize, blobs: &mut Blobs) -> Result<Vec<u8>> {
    const PATH: [u64; 8] = [502, 2, 2, 3, 2, 3, 5, 2];
    if depth == PATH.len() {
        return constant(input, blobs);
    }
    let mut message = fields(input)?;
    if depth == 6 && one(&message, 10)? != b"const" {
        return Ok(input.to_vec());
    }
    if depth == 7 && one(&message, 10)? != b"val" {
        return Ok(input.to_vec());
    }
    for field in &mut message {
        if field.key == PATH[depth] * 8 + 2 {
            field.data = rewrite(&field.data, depth + 1, blobs)?;
        }
    }
    Ok(encode(&message))
}

fn compile(path: &Path) -> Result<PathBuf> {
    use objc2_core_ml::MLModel;
    use objc2_foundation::{NSString, NSURL};
    objc2::rc::autoreleasepool(|_| unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(
            path.to_str().context("invalid Core ML compile path")?,
        ));
        // Synchronous API is available on older supported macOS versions;
        // this runs on the background inference worker, never the UI thread.
        #[allow(deprecated)]
        let compiled =
            MLModel::compileModelAtURL_error(&url).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(PathBuf::from(
            compiled
                .path()
                .context("missing compiled path")?
                .to_string(),
        ))
    })
}

fn compact_model(model: &Path) -> Result<()> {
    let source = model.join("Data/com.microsoft.OnnxRuntime/model.mlmodel");
    if !source.is_file() {
        return Ok(());
    }
    let marker = model.join("schist-binary-weights-v1.sha256");
    if marker.is_file() {
        return Ok(());
    }
    ensure!(
        std::fs::metadata(&source)?.len() <= LIMIT as u64,
        "Core ML source too large"
    );
    let original = std::fs::read(&source)?;
    let hash = crate::sha256_hex(&original);
    let mut blobs = Blobs::new();
    let rewritten = rewrite(&original, 0, &mut blobs)?;
    if blobs.count == 0 {
        std::fs::write(marker, hash)?;
        return Ok(());
    }
    let stage = model.join(format!(".schist-compact-{}.mlpackage", std::process::id()));
    if stage.exists() {
        std::fs::remove_dir_all(&stage)?;
    }
    let result = (|| -> Result<()> {
        let data = stage.join("Data/com.microsoft.OnnxRuntime");
        std::fs::create_dir_all(data.join("weights"))?;
        std::fs::copy(model.join("Manifest.json"), stage.join("Manifest.json"))?;
        for weight in std::fs::read_dir(source.parent().unwrap().join("weights"))? {
            let weight = weight?;
            ensure!(
                weight.file_type()?.is_file(),
                "unexpected Core ML weight entry"
            );
            let target = data.join("weights").join(weight.file_name());
            if std::fs::hard_link(weight.path(), &target).is_err() {
                std::fs::copy(weight.path(), target)?;
            }
        }
        std::fs::write(data.join("model.mlmodel"), rewritten)?;
        std::fs::write(data.join("weights/schist-inline.bin"), &blobs.data)?;
        let compiled = compile(&stage)?;
        let target = model.join("compiled_model.mlmodelc");
        let backup = model.join(".schist-previous.mlmodelc");
        if backup.exists() {
            std::fs::remove_dir_all(&backup)?;
        }
        if target.exists() {
            std::fs::rename(&target, &backup)?;
        }
        if let Err(error) = std::fs::rename(&compiled, &target) {
            if backup.exists() {
                let _ = std::fs::rename(&backup, &target);
            }
            let _ = std::fs::remove_dir_all(&compiled);
            return Err(error.into());
        }
        if backup.exists() {
            std::fs::remove_dir_all(backup)?;
        }
        std::fs::write(marker, hash)?;
        log::info!(target: "schist_neural::execution", "stored {} Core ML constants as binary weights", blobs.count);
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(stage);
    result
}

pub(super) fn compact_cache(root: &Path) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    // Also serialize against another Schist process using this derived cache.
    let lock = std::fs::File::create(root.join("schist-compile.lock"))?;
    lock.lock()?;
    for key in std::fs::read_dir(root)? {
        let key = key?;
        if !key.file_type()?.is_dir() {
            continue;
        }
        for partition in std::fs::read_dir(key.path())? {
            let partition = partition?;
            if partition.file_type()?.is_dir() {
                compact_model(&partition.path().join("model"))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_roundtrip_preserves_unknown_fields_and_rejects_truncation() {
        let source = encode(&[
            integer(901, u64::MAX),
            bytes(123, vec![0, 255, 3]),
            Field {
                key: 37,
                data: vec![0; 4],
            },
        ]);
        assert_eq!(encode(&fields(&source).unwrap()), source);
        assert!(fields(&source[..source.len() - 1]).is_err());
        assert!(fields(&[0xff; 12]).is_err());
        assert!(fields(&[0]).is_err());
    }
    #[test]
    fn blob_storage_preserves_float_bits_and_aligns_records() {
        let data: Vec<_> = [0u32, 0x80000000, 0x7fc12345, 0x3f800000]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let mut b = Blobs::new();
        assert_eq!(b.write(&data).unwrap(), 64);
        assert_eq!(b.write(&data).unwrap(), 192);
        assert_eq!(&b.data[128..144], data);
        assert_eq!(&b.data[256..272], data);
        assert_eq!(&b.data[..8], &[2, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(&b.data[64..72], &[239, 190, 173, 222, 2, 0, 0, 0]);
    }

    #[test]
    fn externalizes_only_exact_float_payload_and_checks_its_shape() {
        let data: Vec<_> = [0u32, 0x80000000, 0x7fc12345, 0x3f800000, 1, 2, 3, 4, 5, 6]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let value = |size| {
            encode(&[
                bytes(1, b"documentation retained".to_vec()),
                bytes(
                    2,
                    encode(&[bytes(
                        1,
                        encode(&[
                            integer(1, 11),
                            integer(2, 1),
                            bytes(3, encode(&[bytes(1, encode(&[integer(1, size)]))])),
                        ]),
                    )]),
                ),
                bytes(
                    3,
                    encode(&[bytes(
                        1,
                        encode(&[bytes(1, encode(&[bytes(1, data.clone())]))]),
                    )]),
                ),
                integer(701, 99),
            ])
        };
        let mut blobs = Blobs::new();
        let result = fields(&constant(&value(10), &mut blobs).unwrap()).unwrap();
        assert_eq!(one(&result, 10).unwrap(), b"documentation retained");
        assert_eq!(one(&result, 701 * 8).unwrap(), [99]);
        assert!(!result.iter().any(|f| f.key == 26));
        assert_eq!(&blobs.data[128..], &data);
        let reference = fields(one(&result, 42).unwrap()).unwrap();
        assert_eq!(
            one(&reference, 10).unwrap(),
            b"@model_path/weights/schist-inline.bin"
        );
        assert_eq!(varint(&mut one(&reference, 16).unwrap()).unwrap(), 64);
        assert!(constant(&value(11), &mut Blobs::new()).is_err());
    }
}
