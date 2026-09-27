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
    matches!(id, "detail-matting" | "subject-guide" | "matting")
}

pub(super) fn archive_size(id: &str) -> Option<u64> {
    bundled(id).then(|| {
        let gpu = format!("{id}-gpu");
        ASSETS
            .iter()
            .filter(|a| a.id == id || a.id == gpu)
            .map(|a| a.archive.len() as u64)
            .sum()
    })
}

fn asset(id: &str, cpu: bool) -> Result<&'static Asset> {
    let id = if cfg!(target_arch = "aarch64") && !cpu && id == "detail-matting" {
        "detail-matting-gpu"
    } else {
        id
    };
    ASSETS
        .iter()
        .find(|a| a.id == id)
        .context("unknown compiled asset")
}

fn cpu_path(path: &Path, fallback: Option<&Asset>) -> Result<PathBuf> {
    fallback
        .map(prepared)
        .unwrap_or_else(|| Ok(path.to_owned()))
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
    input_array: Retained<MLMultiArray>,
    provider: Retained<MLDictionaryFeatureProvider>,
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
            // The GPU detail graph reconstructs only the center consumed by
            // the tiler. CPU fallback retains the original full tile output.
            let output_shape = dimensions(
                &outputs
                    .objectForKey(&output)
                    .and_then(|f| f.multiArrayConstraint())
                    .context("compiled model needs array output")?
                    .shape(),
            )?;
            ensure!(
                output_shape == [1, 1, h, w]
                    || (!cpu && spec.id == "detail-matting" && output_shape == [1, 1, 512, 512]),
                "invalid compiled output extent"
            );
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
            let input_array = MLMultiArray::initWithShape_dataType_error(
                MLMultiArray::alloc(),
                &numbers(&shape),
                MLMultiArrayDataType::Float32,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let value = MLFeatureValue::featureValueWithMultiArray(&input_array);
            let dictionary = NSDictionary::from_slices(
                &[&*input],
                &[value.as_ref() as &objc2::runtime::AnyObject],
            );
            let provider = MLDictionaryFeatureProvider::initWithDictionary_error(
                MLDictionaryFeatureProvider::alloc(),
                &dictionary,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            Ok(Self {
                model,
                input_array,
                provider,
                output,
                shape,
                output_shape,
            })
        })
    }

    pub(super) fn predict(&mut self, inputs: &TVec<TValue>) -> Result<TVec<TValue>> {
        ensure!(
            inputs.len() == 1
                && inputs[0].datum_type() == f32::datum_type()
                && inputs[0].shape() == self.shape,
            "compiled model input differs"
        );
        let view = inputs[0].to_plain_array_view::<f32>()?;
        let input = view.as_slice().context("compiled input not contiguous")?;
        self.predict_parts(&[input])
    }

    fn predict_planes(&mut self, planes: &[&[f32]]) -> Result<TVec<TValue>> {
        ensure!(
            planes.len() == self.shape[1]
                && planes
                    .iter()
                    .all(|p| p.len() == self.shape[2] * self.shape[3]),
            "compiled model planes differ"
        );
        self.predict_parts(planes)
    }

    fn predict_parts(&mut self, parts: &[&[f32]]) -> Result<TVec<TValue>> {
        let count = self.shape.iter().product::<usize>();
        ensure!(
            parts.iter().map(|p| p.len()).sum::<usize>() == count
                && parts.iter().flat_map(|p| p.iter()).all(|v| v.is_finite()),
            "non-finite compiled input"
        );
        autoreleasepool(|_| unsafe {
            // The session owns this allocation and is serialized by Network's
            // mutex. Output is made CPU-accessible before the next call can
            // overwrite it; failed GPU sessions are replaced, not reused.
            let copied = RefCell::new(false);
            self.input_array
                .getMutableBytesWithHandler(&StackBlock::new(
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
                            && size as usize >= count * 4
                            && dimensions(strides.as_ref()).is_ok_and(|s| s == expected)
                        {
                            let mut offset = 0;
                            for input in parts {
                                std::ptr::copy_nonoverlapping(
                                    input.as_ptr(),
                                    ptr.cast::<f32>().as_ptr().add(offset),
                                    input.len(),
                                );
                                offset += input.len();
                            }
                            *copied.borrow_mut() = true;
                        }
                    },
                ));
            ensure!(*copied.borrow(), "invalid compiled input storage");
            let start = Instant::now();
            let output = self
                .model
                .predictionFromFeatures_error(ProtocolObject::from_ref(&*self.provider))
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            if std::env::var_os("SCHIST_NEURAL_NATIVE_TIMING").is_some() {
                log::info!(target: "schist_neural::execution", "compiled {:?} prediction: {:.6}s", self.shape, start.elapsed().as_secs_f64());
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
                        *values.borrow_mut() = copy_output(
                            ptr.cast::<f32>().as_ptr(),
                            size as usize / 4,
                            &self.output_shape,
                            &strides,
                        )
                        .ok();
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

/// # Safety
/// `storage` must cover `capacity` f32 slots, with the logical array elements
/// initialized and readable for this call. Padding is never read or borrowed.
unsafe fn copy_output(
    storage: *const f32,
    capacity: usize,
    shape: &[usize],
    strides: &[usize],
) -> Result<Vec<f32>> {
    ensure!(
        shape.len() == 4 && strides.len() == 4 && shape[0] == 1 && shape[1] == 1,
        "invalid output layout"
    );
    let len = shape[2]
        .checked_mul(shape[3])
        .filter(|&n| n > 0 && n <= 16_777_216)
        .context("invalid output size")?;
    // Nonnegative strides put the furthest element at the bottom right. Check
    // that bound once, then copy contiguous output without an index allocation.
    let last = (shape[2] - 1)
        .checked_mul(strides[2])
        .and_then(|y| {
            (shape[3] - 1)
                .checked_mul(strides[3])
                .and_then(|x| y.checked_add(x))
        })
        .context("compiled output offset overflow")?;
    ensure!(last < capacity, "compiled output exceeds storage");
    if strides[3] == 1 && strides[2] == shape[3] {
        return Ok(unsafe { std::slice::from_raw_parts(storage, len) }.to_vec());
    }
    let mut values = Vec::with_capacity(len);
    for y in 0..shape[2] {
        for x in 0..shape[3] {
            values.push(unsafe { *storage.add(y * strides[2] + x * strides[3]) });
        }
    }
    Ok(values)
}

struct State {
    session: Session,
    cpu: bool,
    last_used: Instant,
}
pub(super) struct Network {
    state: Mutex<State>,
    path: PathBuf,
    fallback: Option<&'static Asset>,
}
impl Network {
    pub(super) fn load(spec: &crate::ModelSpec) -> Result<Self> {
        let selected = asset(spec.id, cpu_requested())?;
        let cpu = asset(spec.id, true)?;
        let fallback = (selected.hash != cpu.hash).then_some(cpu);
        Self::from_paths(spec, prepared(selected)?, fallback)
    }
    #[cfg(feature = "coreml-export")]
    pub(super) fn from_path(spec: &crate::ModelSpec, path: PathBuf) -> Result<Self> {
        Self::from_paths(spec, path, None)
    }
    fn from_paths(
        spec: &crate::ModelSpec,
        path: PathBuf,
        fallback: Option<&'static Asset>,
    ) -> Result<Self> {
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
            Session::load(&cpu_path(&path, fallback)?, spec, true)
        })?;
        start_reaper();
        Ok(Self {
            state: Mutex::new(State {
                session,
                cpu,
                last_used: Instant::now(),
            }),
            path,
            fallback,
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
        self.run_with(spec, |session| session.predict(&inputs))
    }
    pub(super) fn run_planes(
        &self,
        spec: &crate::ModelSpec,
        planes: &[&[f32]],
    ) -> Result<TVec<TValue>> {
        self.run_with(spec, |session| session.predict_planes(planes))
    }
    fn run_with(
        &self,
        spec: &crate::ModelSpec,
        mut predict: impl FnMut(&mut Session) -> Result<TVec<TValue>>,
    ) -> Result<TVec<TValue>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("compiled session lock poisoned"))?;
        state.last_used = Instant::now();
        match predict(&mut state.session) {
            Ok(output) => Ok(output),
            Err(error) if !state.cpu => {
                log::warn!(
                    "{} Core ML GPU prediction failed; using Core ML CPU: {error:#}",
                    spec.id
                );
                state.session = Session::load(&cpu_path(&self.path, self.fallback)?, spec, true)?;
                state.cpu = true;
                predict(&mut state.session)
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
                            let model = entry.get()?.as_ref()?;
                            (std::sync::Arc::strong_count(entry) == 1
                                && std::sync::Arc::strong_count(model) == 1
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
    fn reused_input_storage_observes_new_values_after_rejected_inputs() {
        let spec = crate::spec("matting").unwrap();
        let path = prepared(asset("matting", true).unwrap()).unwrap();
        let mut session = Session::load(&path, spec, true).unwrap();
        let shape = [1, 4, 128, 128];
        let zero = vec![0.0; 128 * 128];
        let one = vec![1.0; 128 * 128];
        let tensor = |value| {
            tvec!(Tensor::from_shape(&shape, &vec![value; 4 * 128 * 128])
                .unwrap()
                .into())
        };
        let assert_value = |outputs: TVec<TValue>, expected: f32| {
            let output = outputs[0].to_plain_array_view::<f32>().unwrap();
            assert_eq!(output.shape(), [1, 1, 128, 128]);
            assert!(output.iter().all(|&v| v == expected));
        };
        assert_value(session.predict(&tensor(0.0)).unwrap(), 0.0);
        assert_value(session.predict_planes(&[one.as_slice(); 4]).unwrap(), 1.0);
        assert!(session.predict(&tvec!()).is_err());
        assert!(session.predict_planes(&[zero.as_slice(); 3]).is_err());
        assert!(session.predict_planes(&[&zero[..127]; 4]).is_err());
        let mut invalid = zero.clone();
        invalid[17] = f32::NAN;
        assert!(session
            .predict_planes(&[&zero, &zero, &invalid, &zero])
            .is_err());
        assert!(session.predict(&tensor(f32::INFINITY)).is_err());
        assert_value(session.predict_planes(&[zero.as_slice(); 4]).unwrap(), 0.0);
        assert_value(session.predict(&tensor(1.0)).unwrap(), 1.0);
        assert_value(session.predict(&tensor(0.0)).unwrap(), 0.0);
    }

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
            for cpu in [false, true] {
                let selected = asset(id, cpu).unwrap();
                let path = prepared(selected).unwrap();
                let mut model = Session::load(&path, spec, cpu).unwrap();
                let actual = model.predict(&tensors).unwrap();
                let actual = actual[0].to_plain_array_view::<f32>().unwrap();
                let (oh, ow) = (actual.shape()[2], actual.shape()[3]);
                let offset = if actual.shape() == reference.shape() {
                    0
                } else {
                    assert_eq!(id, "detail-matting");
                    assert!(!cpu);
                    assert_eq!(actual.shape(), [1, 1, 512, 512]);
                    128
                };
                let mut errors = Vec::with_capacity(oh * ow);
                for y in 0..oh {
                    for x in 0..ow {
                        errors.push(
                            (actual[[0, 0, y, x]] - reference[[0, 0, y + offset, x + offset]])
                                .abs(),
                        );
                    }
                }
                errors.sort_unstable_by(f32::total_cmp);
                let error = *errors.last().unwrap();
                let mean = errors.iter().map(|&v| f64::from(v)).sum::<f64>() / errors.len() as f64;
                let p99 = errors[errors.len() * 99 / 100];
                eprintln!("{id} compiled cpu={cpu}: max={error}, mean={mean}, p99={p99}");
                if selected.id == "detail-matting-gpu" {
                    // Intentional GPU quantization, in units of an 8-bit matte:
                    // average < half a level, 99% < two levels, worst < eight.
                    // CPU fallback and the other models retain strict f32 parity.
                    assert!(error < 8. / 255. && mean < 0.5 / 255. && p99 < 2. / 255.);
                } else {
                    assert!(error < 0.0005, "{id} cpu={cpu} alpha error {error}");
                }
            }
            if id == "detail-matting" {
                // A failed GPU load must use the original full-precision graph,
                // not retry the lower-precision GPU graph on Core ML's CPU.
                let missing = std::env::temp_dir().join(format!(
                    "schist-missing-coreml-{}.mlmodelc",
                    std::process::id()
                ));
                assert!(!missing.exists());
                let fallback = crate::with_adaptive_execution(|| {
                    Network::from_paths(spec, missing, Some(asset(id, true).unwrap())).unwrap()
                });
                assert!(fallback.state.lock().unwrap().cpu);
                let actual = fallback.run(spec, tensors.clone()).unwrap();
                let actual = actual[0].to_plain_array_view::<f32>().unwrap();
                assert!(actual
                    .iter()
                    .zip(reference.iter())
                    .all(|(a, b)| (a - b).abs() < 0.0005));
            }
        }
    }
    #[test]
    fn gpu_detail_asset_has_a_distinct_full_precision_cpu_fallback() {
        assert_eq!(asset("detail-matting", true).unwrap().id, "detail-matting");
        assert_eq!(asset("subject-guide", false).unwrap().id, "subject-guide");
        assert_eq!(asset("matting", false).unwrap().id, "matting");
        let gpu = asset("detail-matting", false).unwrap();
        if cfg!(target_arch = "aarch64") {
            assert_eq!(gpu.id, "detail-matting-gpu");
            assert_ne!(gpu.hash, asset("detail-matting", true).unwrap().hash);
        } else {
            assert_eq!(gpu.id, "detail-matting");
        }
    }
    #[test]
    fn output_layout_handles_padding_and_rejects_out_of_bounds() {
        let storage: Vec<_> = (0..8).map(|n| n as f32).collect();
        let copy = |storage: &[f32], shape: &[usize], strides: &[usize]| unsafe {
            copy_output(storage.as_ptr(), storage.len(), shape, strides)
        };
        assert_eq!(
            copy(&storage, &[1, 1, 2, 3], &[8, 8, 4, 1]).unwrap(),
            [0., 1., 2., 4., 5., 6.]
        );
        assert_eq!(
            copy(&storage, &[1, 1, 2, 2], &[4, 4, 1, 2]).unwrap(),
            [0., 2., 1., 3.]
        );
        assert_eq!(
            copy(&storage, &[1, 1, 2, 3], &[6, 6, 3, 1]).unwrap(),
            storage[..6]
        );
        assert!(copy(&storage[..6], &[1, 1, 2, 3], &[8, 8, 4, 1]).is_err());
        assert!(copy(&storage, &[1, 1, 2, 3], &[8, 8, usize::MAX, 1]).is_err());
        assert!(copy(&storage, &[1, 1, 0, 3], &[8, 8, 4, 1]).is_err());
    }
}
