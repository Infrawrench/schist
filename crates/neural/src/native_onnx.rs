//! Optional Apple Core ML execution for checksum-pinned background models.
//! Derived graphs and device compilation are cached; source weights stay ONNX.
use crate::{Model, ModelSpec};
use anyhow::{ensure, Context, Result};
use ort::{
    ep::{
        self,
        coreml::{ComputeUnits, ModelFormat, SpecializationStrategy},
    },
    session::{builder::GraphOptimizationLevel, Session},
    value::{TensorElementType, TensorRef, ValueType},
};
use prost::Message;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tract_onnx::{pb, prelude::*};

fn cache_version(id: &str) -> &'static str {
    if id == "foreground" {
        "coreml-ort128-v3"
    } else {
        "coreml-ort128-v2"
    }
}

pub(super) fn requested(id: &str) -> bool {
    crate::execution::adaptive_enabled()
        && std::env::var_os("SCHIST_NEURAL_LEGACY_NATIVE").is_none()
        && matches!(
            id,
            "foreground" | "foreground-matting" | "detail-matting" | "subject-guide" | "matting"
        )
}
fn expected_hash(id: &str) -> Option<&'static str> {
    Some(match id {
        "foreground" => "5600024376f572a557870a5eb0afb1e5961636bef4e1e22132025467d0f03333",
        "foreground-matting" => "273501048979b3012b544232234618819225745a13e316b21b4db439a2f28fe8",
        "detail-matting" => "b6240e8404b30bd94c1e84498a03949b7d2e7e891bed85ed06ac1f8ae1d1dc58",
        "subject-guide" => "7a7fc4963357feabd82a3b677824349696d740e31ea0e7c6b249ce9ae632270f",
        "matting" => "368329288b05675c70cc7a13fbcb0845eb1ca98620017e640fd2fc70e267073a",
        _ => return None,
    })
}
fn source_hash(id: &str, compressed: bool) -> Option<&'static str> {
    if !compressed {
        return expected_hash(id);
    }
    Some(match id {
        "detail-matting" => "3c31c66e8ca3a9ec550fec06e2b23c7ac57225fe7a97953e4d8ae38c326a6f37",
        "subject-guide" => "883dd1d4b4c7bdfc24c38ba516f2a299fa0fe1af355f22af4906a1627daa07fa",
        "matting" => "485a9ba783fde2442aa00f52a90a7aa027e086745045f99a16b4ddf4f9b60a2c",
        _ => return None,
    })
}
fn initialize() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        ort::init().with_name("schist-background").commit();
    });
}
fn cache_root(id: &str) -> PathBuf {
    let version = cache_version(id);
    if let Some(path) = std::env::var_os("SCHIST_NEURAL_CACHE") {
        return PathBuf::from(path).join(version);
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Caches")
        });
    base.join("schist/neural").join(version)
}
fn builder() -> Result<ort::session::builder::SessionBuilder> {
    Session::builder()?
        .with_intra_threads(2)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_inter_threads(1)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_intra_op_spinning(false)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_inter_op_spinning(false)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}

/// Offline exporter for the three bundled refiners. Source ONNX is explicitly
/// read from the repository, so enabling this tool cannot add it to the app.
#[cfg(all(target_os = "macos", feature = "coreml-export"))]
pub fn export_coreml_source(id: &str) -> Result<PathBuf> {
    ensure!(
        matches!(id, "detail-matting" | "subject-guide" | "matting"),
        "unsupported compiled export"
    );
    let spec = crate::spec(id).context("unknown export")?;
    let original = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("models")
            .join(spec.file),
    )?;
    ensure!(
        Some(crate::sha256_hex(&original).as_str()) == spec.sha256,
        "source export checksum mismatch"
    );
    initialize();
    let mut path = prepared(
        spec,
        &original,
        expected_hash(id).context("unknown export hash")?,
    )?;
    // BASIC avoids CPU-only activation fusions in the subject guide.
    if id == "subject-guide" {
        let mut proto = pb::ModelProto::decode(std::fs::read(&path)?.as_slice())?;
        crate::native_onnx_graph::guide(&mut proto)?;
        path.set_file_name("coreml-guide.onnx");
        std::fs::write(&path, proto.encode_to_vec())?;
    }
    let cache = path
        .parent()
        .context("missing export parent")?
        .join("compiled-basic-export-v2");
    std::fs::create_dir_all(&cache)?;
    let ep = ep::CoreML::default()
        .with_model_format(ModelFormat::MLProgram)
        .with_compute_units(ComputeUnits::CPUAndGPU)
        .with_static_input_shapes(true)
        .with_low_precision_accumulation_on_gpu(false)
        .with_model_cache_dir(cache.to_string_lossy())
        .build()
        .error_on_failure();
    let session = builder()?
        .with_optimization_level(GraphOptimizationLevel::Level1)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_execution_providers([ep])
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .commit_from_file(path)?;
    drop(session);
    Ok(cache)
}
fn prepared(spec: &ModelSpec, original: &[u8], hash: &str) -> Result<PathBuf> {
    // Bound simultaneous graph conversion/compilation in this process.
    static PREPARE: Mutex<()> = Mutex::new(());
    let _lock = PREPARE
        .lock()
        .map_err(|_| anyhow::anyhow!("native preparation lock poisoned"))?;
    let dir = cache_root(spec.id).join(hash);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("model.onnx");
    let manifest = dir.join("model.sha256");
    if let (Ok(bytes), Ok(digest)) = (std::fs::read(&path), std::fs::read_to_string(&manifest)) {
        if digest.trim() == crate::sha256_hex(&bytes) {
            return Ok(path);
        }
    }
    let start = std::time::Instant::now();
    let raw = crate::decode_model_bytes(original)?;
    ensure!(
        crate::sha256_hex(&raw) == hash,
        "expanded native model checksum mismatch"
    );
    let mut proto = pb::ModelProto::decode(raw.as_ref())?;
    drop(raw);
    let large = matches!(
        spec.id,
        "foreground" | "foreground-matting" | "detail-matting"
    );
    if spec.id.starts_with("foreground") {
        crate::native_onnx_graph::deformable(&mut proto)?;
    }
    let temporary = dir.join(format!("basic-{}.onnx", std::process::id()));
    if large {
        let bytes = proto.encode_to_vec();
        drop(proto);
        let optimized = builder()?
            .with_optimization_level(GraphOptimizationLevel::Level1)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .with_optimized_model_path(&temporary)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .commit_from_memory(&bytes);
        drop(bytes);
        if let Err(e) = optimized {
            let _ = std::fs::remove_file(&temporary);
            return Err(e.into());
        }
        drop(optimized);
        let bytes = std::fs::read(&temporary)?;
        let _ = std::fs::remove_file(&temporary);
        proto = pb::ModelProto::decode(bytes.as_slice())?;
        crate::native_onnx_graph::compact(&mut proto, spec.id)?;
    }
    if spec.id == "foreground" {
        crate::native_onnx_graph::single_parts(&mut proto)?;
    }
    // Core ML's default in-memory/path cache key does not include weight data.
    // Include the source hash and rewrite/runtime version explicitly.
    proto.metadata_props.retain(|p| p.key != "CACHE_KEY");
    proto.metadata_props.push(pb::StringStringEntryProto {
        key: "CACHE_KEY".into(),
        value: crate::sha256_hex(format!("{}:{hash}", cache_version(spec.id)).as_bytes()),
    });
    let bytes = proto.encode_to_vec();
    let digest = crate::sha256_hex(&bytes);
    let temp = dir.join(format!("model-{}.tmp", std::process::id()));
    std::fs::write(&temp, &bytes)?;
    std::fs::rename(&temp, &path)?;
    let temp = dir.join(format!("digest-{}.tmp", std::process::id()));
    std::fs::write(&temp, digest)?;
    std::fs::rename(temp, manifest)?;
    log::info!(target:"schist_neural::execution","{}: prepared native graph in {:.3}s",spec.id,start.elapsed().as_secs_f64());
    Ok(path)
}

// At most the five checksum-pinned background sessions can enter this pool.
// Keep initialized GPU graphs for a short editing session: reloading a compiled
// Core ML model still specializes it, costing seconds even with the disk cache.
const IDLE_TIME: Duration = Duration::from_secs(300);
struct Resident {
    session: Mutex<Session>,
    shape: Vec<usize>,
    output_shape: Vec<usize>,
    last_used: Mutex<Instant>,
    failed: AtomicBool,
}
fn residents() -> &'static Mutex<HashMap<&'static str, Arc<Resident>>> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Arc<Resident>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
fn take_idle(cache: &mut HashMap<&'static str, Arc<Resident>>, now: Instant) -> Vec<Arc<Resident>> {
    let keys: Vec<_> = cache
        .iter()
        .filter(|(_, entry)| {
            Arc::strong_count(entry) == 1
                && (entry.failed.load(Ordering::Relaxed)
                    || entry
                        .last_used
                        .lock()
                        .is_ok_and(|last| now.saturating_duration_since(*last) >= IDLE_TIME))
        })
        .map(|(&key, _)| key)
        .collect();
    keys.into_iter()
        .filter_map(|key| cache.remove(key))
        .collect()
}
fn prune_idle(now: Instant) {
    let mut expired_models = Vec::new();
    if let Ok(mut cache) = crate::cache().write() {
        let keys: Vec<_> = cache
            .iter()
            .filter_map(|(key, entry)| {
                let model = entry.get()?.as_ref()?;
                let native = model.native.as_ref()?;
                (key.starts_with("native:")
                    && Arc::strong_count(entry) == 1
                    && Arc::strong_count(model) == 1
                    && native
                        .resident
                        .last_used
                        .lock()
                        .is_ok_and(|last| now.saturating_duration_since(*last) >= IDLE_TIME))
                .then(|| key.clone())
            })
            .collect();
        for key in keys {
            if let Some(entry) = cache.remove(&key) {
                expired_models.push(entry);
            }
        }
    }
    drop(expired_models);
    let expired = residents()
        .lock()
        .map(|mut cache| take_idle(&mut cache, now))
        .unwrap_or_default();
    // Device destruction can be slow; never hold the pool lock while dropping.
    drop(expired);
}
fn start_reaper() {
    static START: OnceLock<()> = OnceLock::new();
    START.get_or_init(|| {
        if let Err(error) = std::thread::Builder::new()
            .name("neural-idle-cache".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_secs(60));
                prune_idle(Instant::now());
            })
        {
            log::warn!("native model idle cleanup: {error}");
        }
    });
}
pub(super) struct Network {
    resident: Arc<Resident>,
    source: Vec<u8>,
    fallback: OnceLock<std::result::Result<Box<Model>, String>>,
}
impl Network {
    pub(super) fn load(spec: &ModelSpec, original: &[u8]) -> Result<Self> {
        let expected = expected_hash(spec.id).context("model not supported by native runtime")?;
        let source = source_hash(spec.id, original.starts_with(b"\xfd7zXZ\0"))
            .context("unsupported native model archive")?;
        ensure!(
            crate::sha256_hex(original) == source,
            "native model checksum mismatch"
        );
        static LOAD: Mutex<()> = Mutex::new(());
        let _lock = LOAD
            .lock()
            .map_err(|_| anyhow::anyhow!("native load lock poisoned"))?;
        prune_idle(Instant::now());
        if let Some(resident) = residents()
            .lock()
            .ok()
            .and_then(|cache| cache.get(expected).cloned())
        {
            if !resident.failed.load(Ordering::Relaxed) {
                *resident
                    .last_used
                    .lock()
                    .map_err(|_| anyhow::anyhow!("native timestamp lock poisoned"))? =
                    Instant::now();
                log::info!(target: "schist_neural::execution", "{}: reused native GPU session", spec.id);
                return Ok(Self {
                    resident,
                    source: original.into(),
                    fallback: OnceLock::new(),
                });
            }
        }
        initialize();
        let path = prepared(spec, original, expected)?;
        let cache = path
            .parent()
            .context("missing model directory")?
            .join("compiled");
        std::fs::create_dir_all(&cache)?;
        // Existing downloaded-model caches may contain hundreds of MB of
        // hexadecimal weight text. Recompile those constants into exact binary
        // storage before Core ML loads the cached graph again.
        if let Err(error) = crate::coreml_weights::compact_cache(&cache) {
            log::warn!(target: "schist_neural::execution", "{} compact Core ML cache unavailable: {error:#}", spec.id);
        }
        let ep = ep::CoreML::default()
            .with_model_format(ModelFormat::MLProgram)
            .with_compute_units(if cfg!(all(target_os = "ios", target_abi = "sim")) {
                ComputeUnits::CPUOnly
            } else {
                ComputeUnits::CPUAndGPU
            })
            .with_static_input_shapes(true)
            .with_low_precision_accumulation_on_gpu(false)
            .with_model_cache_dir(cache.to_string_lossy());
        // Optimization hints arrived in iOS 18; keep iOS 16/17 usable.
        let ep = if crate::native_coreml::supports_fast_prediction() {
            ep.with_specialization_strategy(SpecializationStrategy::FastPrediction)
        } else {
            ep
        };
        let ep = ep.build().error_on_failure();
        let session = builder()?
            .with_execution_providers([ep])
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .commit_from_file(&path)?;
        ensure!(
            session.inputs().len() == 1 && session.outputs().len() == 1,
            "invalid native model interface"
        );
        let ValueType::Tensor {
            ty: TensorElementType::Float32,
            shape,
            ..
        } = session.inputs()[0].dtype()
        else {
            anyhow::bail!("invalid native input type");
        };
        let shape: Vec<usize> = shape
            .iter()
            .map(|&v| usize::try_from(v))
            .collect::<std::result::Result<_, _>>()?;
        let (w, h) = spec.input.dims();
        let channels = if matches!(spec.id, "detail-matting" | "matting") {
            4
        } else {
            3
        };
        ensure!(shape == [1, channels, h, w], "invalid native input shape");
        let ValueType::Tensor {
            ty: TensorElementType::Float32,
            shape: output,
            ..
        } = session.outputs()[0].dtype()
        else {
            anyhow::bail!("invalid native output type");
        };
        let output_shape: Vec<usize> = output
            .iter()
            .map(|&v| usize::try_from(v))
            .collect::<std::result::Result<_, _>>()?;
        ensure!(output_shape == [1, 1, h, w], "invalid native output shape");
        log::info!(target:"schist_neural::execution","{}: native Core ML GPU graph",spec.id);
        let resident = Arc::new(Resident {
            session: Mutex::new(session),
            shape,
            output_shape,
            last_used: Mutex::new(Instant::now()),
            failed: AtomicBool::new(false),
        });
        // A phone releases each detector before loading the next stage. Keep
        // the compiled disk cache, but never retain a second owning session.
        if !cfg!(target_os = "ios") {
            residents()
                .lock()
                .map_err(|_| anyhow::anyhow!("native cache lock poisoned"))?
                .insert(expected, resident.clone());
            start_reaper();
        }
        Ok(Self {
            resident,
            source: original.into(),
            fallback: OnceLock::new(),
        })
    }
    pub(super) fn channels(&self) -> usize {
        self.resident.shape[1]
    }
    pub(super) fn active(&self) -> bool {
        !self.resident.failed.load(Ordering::Relaxed)
    }
    fn predict(&self, inputs: &TVec<TValue>) -> Result<TVec<TValue>> {
        ensure!(
            inputs.len() == 1
                && inputs[0].datum_type() == f32::datum_type()
                && inputs[0].shape() == self.resident.shape,
            "native model input differs"
        );
        let view = inputs[0].to_plain_array_view::<f32>()?;
        let input = view.as_slice().context("native input not contiguous")?;
        ensure!(
            input.iter().all(|v| v.is_finite()),
            "non-finite native input"
        );
        let tensor = TensorRef::from_array_view((self.resident.shape.as_slice(), input))?;
        let mut session = self
            .resident
            .session
            .lock()
            .map_err(|_| anyhow::anyhow!("native session lock poisoned"))?;
        let start = Instant::now();
        let outputs = session.run(ort::inputs![tensor])?;
        if std::env::var_os("SCHIST_NEURAL_NATIVE_TIMING").is_some() {
            log::info!(target: "schist_neural::execution", "native {:?} prediction: {:.3}s", self.resident.shape, start.elapsed().as_secs_f64());
        }
        if let Ok(mut last) = self.resident.last_used.lock() {
            *last = Instant::now();
        }
        let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
        let shape: Vec<usize> = shape
            .iter()
            .map(|&v| usize::try_from(v))
            .collect::<std::result::Result<_, _>>()?;
        let len = shape
            .iter()
            .try_fold(1usize, |n, &v| n.checked_mul(v))
            .context("native output overflow")?;
        ensure!(
            shape == self.resident.output_shape
                && len == data.len()
                && len > 0
                && len <= 16_777_216
                && data.iter().all(|v| v.is_finite()),
            "invalid native output"
        );
        Ok(tvec!(Tensor::from_shape(&shape, data)?.into()))
    }
    pub(super) fn run(
        &self,
        spec: &'static ModelSpec,
        inputs: TVec<TValue>,
    ) -> Result<TVec<TValue>> {
        if !self.resident.failed.load(Ordering::Relaxed) {
            match self.predict(&inputs) {
                Ok(output) => return Ok(output),
                Err(error) => {
                    self.resident.failed.store(true, Ordering::Relaxed);
                    log::warn!(
                        target: "schist_neural::execution",
                        "{} native inference failed; using tract: {error:#}",
                        spec.id
                    );
                }
            }
        }
        let model = self
            .fallback
            .get_or_init(|| {
                Model::from_bytes_inner(spec, &self.source, false)
                    .map(Box::new)
                    .map_err(|e| format!("{e:#}"))
            })
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.clone()))?;
        model.run_cpu(inputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unpinned_model_before_runtime_or_cache_access() {
        let spec = crate::spec("detail-matting").unwrap();
        assert!(Network::load(spec, b"invalid model")
            .err()
            .unwrap()
            .to_string()
            .contains("checksum"));
        assert!(Network::load(crate::spec("detail").unwrap(), b"invalid model").is_err());
    }

    #[test]
    fn idle_pool_keeps_in_flight_sessions_and_releases_unused_models() {
        initialize();
        let raw = crate::decode_model_bytes(crate::MATTING_ONNX_XZ).unwrap();
        let make = || {
            Arc::new(Resident {
                session: Mutex::new(builder().unwrap().commit_from_memory(&raw).unwrap()),
                shape: vec![1, 4, 128, 128],
                output_shape: vec![1, 1, 128, 128],
                last_used: Mutex::new(Instant::now()),
                failed: AtomicBool::new(false),
            })
        };
        let active = make();
        let inactive = make();
        let weak = Arc::downgrade(&inactive);
        let mut cache = HashMap::from([("active", active.clone()), ("inactive", inactive)]);
        let now = Instant::now();
        assert!(take_idle(&mut cache, now).is_empty());
        drop(take_idle(
            &mut cache,
            now + IDLE_TIME + Duration::from_secs(1),
        ));
        assert!(weak.upgrade().is_none());
        assert!(cache.contains_key("active"));
        drop(active);
        drop(take_idle(
            &mut cache,
            now + IDLE_TIME + Duration::from_secs(1),
        ));
        assert!(cache.is_empty());
    }

    #[test]
    fn idle_model_cache_drops_source_buffers_but_keeps_active_owners() {
        initialize();
        let raw = crate::decode_model_bytes(crate::MATTING_ONNX_XZ).unwrap();
        let now = Instant::now();
        let resident = Arc::new(Resident {
            session: Mutex::new(builder().unwrap().commit_from_memory(&raw).unwrap()),
            shape: vec![1, 4, 128, 128],
            output_shape: vec![1, 1, 128, 128],
            last_used: Mutex::new(now - IDLE_TIME - Duration::from_secs(1)),
            failed: AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&resident);
        let model = Arc::new(Model {
            compiled: None,
            native: Some(Network {
                resident: resident.clone(),
                source: raw.into_owned(),
                fallback: OnceLock::new(),
            }),
            plan: None,
            gpu: None,
            partitioned: None,
            channels: 4,
            nhwc: false,
            restoration_max_side: None,
            restoration_tile_size: None,
            restoration_halo_cleanup: false,
            spec: crate::spec("matting").unwrap(),
        });
        residents()
            .lock()
            .unwrap()
            .insert("idle-cache-test", resident);
        crate::cache().write().unwrap().insert(
            "native:idle-cache-test".into(),
            Arc::new(OnceLock::from(Some(model.clone()))),
        );
        prune_idle(now);
        assert!(weak.upgrade().is_some());
        assert!(crate::cache()
            .read()
            .unwrap()
            .contains_key("native:idle-cache-test"));
        drop(model);
        // A caller can have obtained the slot but not yet cloned its model.
        // Keep that handoff alive just like an already returned Model owner.
        let loading = crate::cache().read().unwrap()["native:idle-cache-test"].clone();
        prune_idle(now);
        assert!(weak.upgrade().is_some());
        assert!(crate::cache()
            .read()
            .unwrap()
            .contains_key("native:idle-cache-test"));
        drop(loading);
        prune_idle(now);
        assert!(weak.upgrade().is_none());
        assert!(!crate::cache()
            .read()
            .unwrap()
            .contains_key("native:idle-cache-test"));
    }

    /// Requires the two pinned detector exports installed locally and a Core ML
    /// GPU. Includes graph preparation when uncached; deliberately opt-in.
    #[test]
    #[ignore = "requires installed background detectors and Apple Silicon GPU"]
    fn native_background_models_match_original_cpu_graphs() {
        initialize();
        for id in ["foreground-matting", "foreground"] {
            let spec = crate::spec(id).unwrap();
            let original = match id {
                "subject-guide" => crate::SUBJECT_GUIDE_ONNX_XZ.to_vec(),
                "detail-matting" => crate::DETAIL_MATTING_ONNX_XZ.to_vec(),
                "matting" => crate::MATTING_ONNX_XZ.to_vec(),
                _ => std::fs::read(crate::model_dir().join(spec.file))
                    .expect("install pinned detector first"),
            };
            let native = Network::load(spec, &original).unwrap();
            let reused = Network::load(spec, &original).unwrap();
            assert!(Arc::ptr_eq(&native.resident, &reused.resident));
            drop(reused);
            let input: Vec<f32> = (0..native.resident.shape.iter().product())
                .map(|i| {
                    let value = ((i * 1664525usize + 1013904223) % 65536) as f32 / 65535.0;
                    if native.channels() == 3 {
                        value * 2.0 - 1.0
                    } else {
                        value
                    }
                })
                .collect();
            let raw = crate::decode_model_bytes(&original).unwrap();
            let mut cpu = builder().unwrap().commit_from_memory(&raw).unwrap();
            drop(raw);
            let tensor =
                TensorRef::from_array_view((native.resident.shape.as_slice(), input.as_slice()))
                    .unwrap();
            let outputs = cpu.run(ort::inputs![tensor]).unwrap();
            let (shape, reference) = outputs[0].try_extract_tensor::<f32>().unwrap();
            let reference = reference.to_vec();
            assert_eq!(
                shape.iter().map(|&v| v as usize).collect::<Vec<_>>(),
                native.resident.output_shape
            );
            drop(outputs);
            drop(cpu);
            let actual = native
                .predict(&tvec!(Tensor::from_shape(&native.resident.shape, &input)
                    .unwrap()
                    .into()))
                .unwrap();
            let actual = actual[0].to_plain_array_view::<f32>().unwrap();
            let error = actual
                .iter()
                .zip(&reference)
                .map(|(&a, &b)| {
                    if id.starts_with("foreground") {
                        ((1.0 / (1.0 + (-a).exp())) - (1.0 / (1.0 + (-b).exp()))).abs()
                    } else {
                        (a - b).abs()
                    }
                })
                .fold(0.0f32, f32::max);
            eprintln!("{id}: maximum alpha error {error}");
            assert!(error < 0.0005, "{id} alpha error {error}");
        }
    }
}
