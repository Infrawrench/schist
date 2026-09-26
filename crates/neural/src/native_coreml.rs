//! Compressed, precompiled macOS background refiners. No ONNX copy is linked.
use anyhow::{ensure, Context, Result};
use block2::StackBlock;
use objc2::{
    rc::{autoreleasepool, Retained},
    runtime::ProtocolObject,
    AnyThread,
};
use objc2_core_ml::*;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};
use std::{
    cell::RefCell,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tract_onnx::prelude::*;

struct Asset {
    id: &'static str,
    archive: &'static [u8],
    hash: &'static str,
    files: &'static [(&'static str, u64, &'static str)],
}
include!("native_coreml_assets.rs");

pub(super) fn bundled(id: &str) -> bool {
    ASSETS.iter().any(|asset| asset.id == id)
}

pub(super) fn archive_size(id: &str) -> Option<u64> {
    ASSETS
        .iter()
        .find(|asset| asset.id == id)
        .map(|asset| asset.archive.len() as u64)
}

fn cpu_requested() -> bool {
    !crate::execution::adaptive_enabled() || std::env::var_os("SCHIST_NEURAL_COREML_CPU").is_some()
}

pub(super) fn cache_key(id: &str) -> String {
    format!(
        "compiled:{}:{id}",
        if cpu_requested() { "cpu" } else { "gpu" }
    )
}

fn root() -> PathBuf {
    let base = std::env::var_os("SCHIST_NEURAL_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                        .join("Library/Caches")
                })
                .join("schist/neural")
        });
    base.join("coreml-bundled-v1")
}

fn valid(path: &Path, asset: &Asset) -> bool {
    asset.files.iter().all(|(name, size, hash)| {
        let path = path.join(name);
        std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() == *size)
            && std::fs::read(path).is_ok_and(|bytes| crate::sha256_hex(&bytes) == *hash)
    })
}

fn extract(asset: &Asset, directory: &Path) -> Result<()> {
    ensure!(
        crate::sha256_hex(asset.archive) == asset.hash,
        "compiled model archive checksum mismatch"
    );
    let decoder = lzma_rust2::XzReader::new(asset.archive, false);
    let mut archive = tar::Archive::new(decoder.take(128 * 1024 * 1024));
    let mut seen = std::collections::HashSet::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        ensure!(
            entry.header().entry_type().is_file(),
            "compiled archive contains a non-file entry"
        );
        let path = entry.path()?.into_owned();
        let name = path.to_str().context("invalid compiled archive path")?;
        let (_, size, hash) = asset
            .files
            .iter()
            .find(|(allowed, _, _)| *allowed == name)
            .context("unexpected compiled archive member")?;
        ensure!(
            seen.insert(name.to_owned()) && entry.size() == *size,
            "duplicate or oversized compiled file"
        );
        ensure!(
            path.components()
                .all(|p| matches!(p, std::path::Component::Normal(_))),
            "unsafe compiled archive path"
        );
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        ensure!(
            crate::sha256_hex(&bytes) == *hash,
            "compiled file checksum mismatch"
        );
        let target = directory.join(path);
        std::fs::create_dir_all(target.parent().context("missing compiled directory")?)?;
        std::fs::write(target, bytes)?;
    }
    ensure!(
        seen.len() == asset.files.len(),
        "incomplete compiled archive"
    );
    // tar stops at its end marker; drain XZ too so its footer/checksum and the
    // expansion bound are checked, including on a truncated cached archive.
    let mut stream = archive.into_inner();
    std::io::copy(&mut stream, &mut std::io::sink())?;
    ensure!(
        stream.limit() > 0,
        "compiled archive exceeds expansion bound"
    );
    Ok(())
}

fn prepared(asset: &Asset) -> Result<PathBuf> {
    static PREPARE: Mutex<()> = Mutex::new(());
    let _guard = PREPARE
        .lock()
        .map_err(|_| anyhow::anyhow!("compiled archive lock poisoned"))?;
    let root = root();
    std::fs::create_dir_all(&root)?;
    // Another app process may be extracting the same content-addressed asset.
    let lock = std::fs::File::create(root.join("extract.lock"))?;
    lock.lock()?;
    let target = root.join(format!("{}-{}.mlmodelc", asset.id, asset.hash));
    if valid(&target, asset) {
        return Ok(target);
    }
    let temp = root.join(format!(
        ".extract-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir(&temp)?;
    let result = (|| {
        extract(asset, &temp)?;
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::rename(&temp, &target)?;
        Ok(target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(temp);
    }
    result
}

fn numbers(values: &[usize]) -> Retained<NSArray<NSNumber>> {
    NSArray::from_retained_slice(
        &values
            .iter()
            .map(|&v| NSNumber::new_usize(v))
            .collect::<Vec<_>>(),
    )
}
fn dimensions(values: &NSArray<NSNumber>) -> Result<Vec<usize>> {
    values
        .iter()
        .map(|v| usize::try_from(v.as_i64()).map_err(Into::into))
        .collect()
}

pub(super) struct Session {
    model: Retained<MLModel>,
    input: Retained<NSString>,
    output: Retained<NSString>,
    shape: Vec<usize>,
    output_shape: Vec<usize>,
}
// Core ML supports background-thread prediction. Ownership may move between
// workers, but every access to this model and its Foundation objects is under
// Network's mutex. None of these objects escapes the synchronous call.
unsafe impl Send for Session {}

impl Session {
    pub(super) fn load(path: &Path, spec: &crate::ModelSpec, cpu: bool) -> Result<Self> {
        autoreleasepool(|_| unsafe {
            let config = MLModelConfiguration::new();
            config.setComputeUnits(if cpu {
                MLComputeUnits::CPUOnly
            } else {
                MLComputeUnits::CPUAndGPU
            });
            config.setAllowLowPrecisionAccumulationOnGPU(false);
            use objc2::runtime::NSObjectProtocol;
            if config.respondsToSelector(objc2::sel!(setOptimizationHints:)) {
                let hints = MLOptimizationHints::new();
                hints.setSpecializationStrategy(MLSpecializationStrategy::FastPrediction);
                config.setOptimizationHints(&hints);
            }
            let url = NSURL::fileURLWithPath_isDirectory(
                &NSString::from_str(path.to_str().context("invalid compiled path")?),
                true,
            );
            let model = MLModel::modelWithContentsOfURL_configuration_error(&url, &config)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let description = model.modelDescription();
            let inputs = description.inputDescriptionsByName();
            let outputs = description.outputDescriptionsByName();
            ensure!(
                inputs.len() == 1 && outputs.len() == 1,
                "invalid compiled model interface"
            );
            let input = inputs.allKeys().objectAtIndex(0);
            let output = outputs.allKeys().objectAtIndex(0);
            let (w, h) = spec.input.dims();
            let shape = vec![
                1,
                if matches!(spec.id, "matting" | "detail-matting") {
                    4
                } else {
                    3
                },
                h,
                w,
            ];
            let output_shape = vec![1, 1, h, w];
            for (features, name, expected) in [
                (&inputs, &input, &shape),
                (&outputs, &output, &output_shape),
            ] {
                let constraint = features
                    .objectForKey(name)
                    .and_then(|f| f.multiArrayConstraint())
                    .context("compiled model needs array feature")?;
                ensure!(
                    constraint.dataType() == MLMultiArrayDataType::Float32
                        && dimensions(&constraint.shape())? == *expected,
                    "invalid compiled model shape or precision"
                );
            }
            Ok(Self {
                model,
                input,
                output,
                shape,
                output_shape,
            })
        })
    }

    pub(super) fn predict(&self, inputs: &TVec<TValue>) -> Result<TVec<TValue>> {
        ensure!(
            inputs.len() == 1
                && inputs[0].datum_type() == f32::datum_type()
                && inputs[0].shape() == self.shape,
            "compiled model input differs"
        );
        let view = inputs[0].to_plain_array_view::<f32>()?;
        let input = view.as_slice().context("compiled input not contiguous")?;
        ensure!(
            input.iter().all(|v| v.is_finite()),
            "non-finite compiled input"
        );
        autoreleasepool(|_| unsafe {
            // Core ML owns this allocation, including while a GPU command uses
            // it. We never lend an immutable Rust allocation as writable memory.
            let array = MLMultiArray::initWithShape_dataType_error(
                MLMultiArray::alloc(),
                &numbers(&self.shape),
                MLMultiArrayDataType::Float32,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let copied = RefCell::new(false);
            array.getMutableBytesWithHandler(&StackBlock::new(
                |ptr: std::ptr::NonNull<std::ffi::c_void>,
                 size: isize,
                 strides: std::ptr::NonNull<NSArray<NSNumber>>| {
                    let expected = vec![
                        self.shape[1] * self.shape[2] * self.shape[3],
                        self.shape[2] * self.shape[3],
                        self.shape[3],
                        1,
                    ];
                    if size >= 0
                        && size as usize >= input.len() * 4
                        && dimensions(strides.as_ref()).is_ok_and(|s| s == expected)
                    {
                        std::ptr::copy_nonoverlapping(
                            input.as_ptr(),
                            ptr.cast::<f32>().as_ptr(),
                            input.len(),
                        );
                        *copied.borrow_mut() = true;
                    }
                },
            ));
            ensure!(*copied.borrow(), "invalid compiled input storage");
            let value = MLFeatureValue::featureValueWithMultiArray(&array);
            let dictionary = NSDictionary::from_slices(
                &[&*self.input],
                &[value.as_ref() as &objc2::runtime::AnyObject],
            );
            let provider = MLDictionaryFeatureProvider::initWithDictionary_error(
                MLDictionaryFeatureProvider::alloc(),
                &dictionary,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let start = Instant::now();
            let output = self
                .model
                .predictionFromFeatures_error(ProtocolObject::from_ref(&*provider))
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            if std::env::var_os("SCHIST_NEURAL_NATIVE_TIMING").is_some() {
                log::info!(target: "schist_neural::execution", "compiled {:?} prediction: {:.3}s", self.shape, start.elapsed().as_secs_f64());
            }
            let array = output
                .featureValueForName(&self.output)
                .and_then(|v| v.multiArrayValue())
                .context("missing compiled output")?;
            ensure!(
                array.dataType() == MLMultiArrayDataType::Float32
                    && dimensions(&array.shape())? == self.output_shape,
                "invalid compiled output shape or precision"
            );
            let values = RefCell::new(None);
            array.getBytesWithHandler(&StackBlock::new(
                |ptr: std::ptr::NonNull<std::ffi::c_void>, size: isize| {
                    if size < 0 {
                        return;
                    }
                    // Strides are read inside the block: Core ML can change storage
                    // when making a GPU result CPU-accessible.
                    if let Ok(strides) = dimensions(&array.strides()) {
                        if let Ok(offsets) =
                            output_offsets(&self.output_shape, &strides, size as usize / 4)
                        {
                            let base = ptr.cast::<f32>().as_ptr();
                            *values.borrow_mut() = Some(
                                offsets
                                    .into_iter()
                                    .map(|i| *base.add(i))
                                    .collect::<Vec<_>>(),
                            );
                        }
                    }
                },
            ));
            let values = values
                .into_inner()
                .context("invalid compiled output storage")?;
            ensure!(
                values.iter().all(|v| v.is_finite()),
                "non-finite compiled output"
            );
            Ok(tvec!(
                Tensor::from_shape(&self.output_shape, &values)?.into()
            ))
        })
    }
}

fn output_offsets(shape: &[usize], strides: &[usize], capacity: usize) -> Result<Vec<usize>> {
    ensure!(
        shape.len() == 4 && strides.len() == 4 && shape[0] == 1 && shape[1] == 1,
        "invalid output layout"
    );
    let len = shape[2]
        .checked_mul(shape[3])
        .filter(|&n| n > 0 && n <= 16_777_216)
        .context("invalid output size")?;
    let mut offsets = Vec::with_capacity(len);
    for y in 0..shape[2] {
        for x in 0..shape[3] {
            offsets.push(
                y.checked_mul(strides[2])
                    .and_then(|y| x.checked_mul(strides[3]).and_then(|x| y.checked_add(x)))
                    .filter(|&i| i < capacity)
                    .context("compiled output exceeds storage")?,
            );
        }
    }
    Ok(offsets)
}

struct State {
    session: Session,
    cpu: bool,
    last_used: Instant,
}
pub(super) struct Network {
    state: Mutex<State>,
    path: PathBuf,
}
impl Network {
    pub(super) fn load(spec: &crate::ModelSpec) -> Result<Self> {
        let asset = ASSETS
            .iter()
            .find(|a| a.id == spec.id)
            .context("unknown compiled asset")?;
        let path = prepared(asset)?;
        let mut cpu = cpu_requested();
        let session = Session::load(&path, spec, cpu).or_else(|error| {
            if cpu {
                return Err(error);
            }
            log::warn!(
                "{} Core ML GPU load failed; using Core ML CPU: {error:#}",
                spec.id
            );
            cpu = true;
            Session::load(&path, spec, true)
        })?;
        start_reaper();
        Ok(Self {
            state: Mutex::new(State {
                session,
                cpu,
                last_used: Instant::now(),
            }),
            path,
        })
    }
    pub(super) fn idle(&self, now: Instant) -> bool {
        self.state
            .try_lock()
            .is_ok_and(|s| now.saturating_duration_since(s.last_used) >= Duration::from_secs(300))
    }
    pub(super) fn run(
        &self,
        spec: &crate::ModelSpec,
        inputs: TVec<TValue>,
    ) -> Result<TVec<TValue>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("compiled session lock poisoned"))?;
        state.last_used = Instant::now();
        match state.session.predict(&inputs) {
            Ok(output) => Ok(output),
            Err(error) if !state.cpu => {
                log::warn!(
                    "{} Core ML GPU prediction failed; using Core ML CPU: {error:#}",
                    spec.id
                );
                state.session = Session::load(&self.path, spec, true)?;
                state.cpu = true;
                state.session.predict(&inputs)
            }
            Err(error) => Err(error),
        }
    }
}
fn start_reaper() {
    static START: OnceLock<()> = OnceLock::new();
    START.get_or_init(|| {
        if let Err(error) = std::thread::Builder::new()
            .name("coreml-idle-cache".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_secs(60));
                let expired = if let Ok(mut cache) = crate::cache().write() {
                    let now = Instant::now();
                    let keys: Vec<_> = cache
                        .iter()
                        .filter_map(|(key, entry)| {
                            let model = entry.as_ref()?;
                            (std::sync::Arc::strong_count(model) == 1
                                && model.compiled.as_ref().is_some_and(|m| m.idle(now)))
                            .then(|| key.clone())
                        })
                        .collect();
                    keys.into_iter()
                        .filter_map(|key| cache.remove(&key))
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                drop(expired);
            })
        {
            log::warn!("compiled model idle cleanup: {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_checks_members_contents_and_truncated_xz_footer() {
        use std::io::Write;
        let directory =
            std::env::temp_dir().join(format!("schist-coreml-archive-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let data = b"compiled test data";
        let hash: &'static str = Box::leak(crate::sha256_hex(data).into_boxed_str());
        let expected: &'static [(&'static str, u64, &'static str)] =
            Box::leak(vec![("model.mil", data.len() as u64, hash)].into_boxed_slice());
        let make = |name: &str, duplicate: bool, truncated: bool| {
            let mut tar = tar::Builder::new(Vec::new());
            for _ in 0..if duplicate { 2 } else { 1 } {
                let mut header = tar::Header::new_ustar();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(&mut header, name, &data[..]).unwrap();
            }
            let raw = tar.into_inner().unwrap();
            let mut xz =
                lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(0))
                    .unwrap();
            xz.write_all(&raw).unwrap();
            let mut compressed = xz.finish().unwrap();
            if truncated {
                compressed.truncate(compressed.len() - 12);
            }
            Asset {
                id: "test",
                hash: Box::leak(crate::sha256_hex(&compressed).into_boxed_str()),
                archive: Box::leak(compressed.into_boxed_slice()),
                files: expected,
            }
        };
        let asset = make("model.mil", false, false);
        extract(&asset, &directory).unwrap();
        assert!(valid(&directory, &asset));
        std::fs::write(directory.join("model.mil"), b"changed").unwrap();
        assert!(!valid(&directory, &asset));
        assert!(extract(&make("extra.bin", false, false), &directory).is_err());
        assert!(extract(&make("model.mil", true, false), &directory).is_err());
        assert!(extract(&make("model.mil", false, true), &directory).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "full Core ML CPU/GPU parity; run make check-background-removal-native"]
    fn compiled_background_models_match_original_cpu_graphs() {
        for (id, original) in [
            ("matting", crate::MATTING_ONNX_XZ),
            ("subject-guide", crate::SUBJECT_GUIDE_ONNX_XZ),
            ("detail-matting", crate::DETAIL_MATTING_ONNX_XZ),
        ] {
            let spec = crate::spec(id).unwrap();
            let (w, h) = spec.input.dims();
            let channels = if id == "subject-guide" { 3 } else { 4 };
            let shape = [1, channels, h, w];
            let input: Vec<_> = (0..shape.iter().product())
                .map(|i: usize| ((i * 1664525 + 1013904223) % 65536) as f32 / 65535.)
                .collect();
            let tensors = tvec!(Tensor::from_shape(&shape, &input).unwrap().into());
            let reference = {
                let model = crate::Model::from_bytes_inner(spec, original, false).unwrap();
                model.run_cpu(tensors.clone()).unwrap()
            };
            let reference = reference[0].to_plain_array_view::<f32>().unwrap();
            let path = prepared(ASSETS.iter().find(|a| a.id == id).unwrap()).unwrap();
            for cpu in [false, true] {
                let model = Session::load(&path, spec, cpu).unwrap();
                let actual = model.predict(&tensors).unwrap();
                let actual = actual[0].to_plain_array_view::<f32>().unwrap();
                let error = actual
                    .iter()
                    .zip(reference.iter())
                    .map(|(a, b)| (a - b).abs())
                    .fold(0f32, f32::max);
                eprintln!("{id} compiled cpu={cpu}: maximum alpha error {error}");
                assert!(error < 0.0005, "{id} cpu={cpu} alpha error {error}");
            }
        }
    }
    #[test]
    fn output_layout_handles_padding_and_rejects_out_of_bounds() {
        assert_eq!(
            output_offsets(&[1, 1, 2, 3], &[8, 8, 4, 1], 8).unwrap(),
            [0, 1, 2, 4, 5, 6]
        );
        assert_eq!(
            output_offsets(&[1, 1, 2, 2], &[4, 4, 1, 2], 4).unwrap(),
            [0, 2, 1, 3]
        );
        assert!(output_offsets(&[1, 1, 2, 3], &[8, 8, 4, 1], 6).is_err());
        assert!(output_offsets(&[1, 1, 2, 3], &[8, 8, usize::MAX, 1], 6).is_err());
        assert!(output_offsets(&[1, 1, 0, 3], &[8, 8, 4, 1], 6).is_err());
    }
}
