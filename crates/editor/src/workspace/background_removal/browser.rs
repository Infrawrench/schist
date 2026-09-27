//! Browser transport only. The actual inference is the same Rust pipeline as
//! native, in a separate WASM instance so blocking kernels cannot freeze GPUI.
use super::*;
use futures::{select_biased, FutureExt as _};
use js_sys::{Float32Array, Function, Promise, Reflect};
use std::sync::atomic::AtomicUsize;
use wasm_bindgen::{closure::Closure, prelude::*};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = window, js_name = __schistBackgroundRemoval)]
    fn start(
        models: &str,
        rgb: Float32Array,
        width: usize,
        height: usize,
        progress: &Function,
    ) -> Result<JsValue, JsValue>;
}

fn js_error(error: JsValue) -> anyhow::Error {
    anyhow::anyhow!("background-removal worker: {error:?}")
}

struct Worker(JsValue);
impl Drop for Worker {
    fn drop(&mut self) {
        if let Ok(cancel) = Reflect::get(&self.0, &"cancel".into()) {
            if let Some(cancel) = cancel.dyn_ref::<Function>() {
                let _ = cancel.call0(&self.0);
            }
        }
    }
}

pub(super) async fn run(
    session: &Session,
    cancelled: &AtomicBool,
    duplicate_name: &str,
    executor: gpui::BackgroundExecutor,
    mut progress: impl FnMut(Option<&'static str>) -> bool,
) -> anyhow::Result<schist_core::automatic_mask::Prepared> {
    if cancelled.load(Ordering::Relaxed) {
        anyhow::bail!("cancelled");
    }
    let models: Vec<_> = schist_neural::BACKGROUND_REMOVAL_MODELS
        .iter()
        .map(|id| {
            let spec = schist_neural::spec(id).expect("background model catalogue");
            serde_json::json!({ "id": id, "url": schist_neural::download_url(spec) })
        })
        .collect();
    let phase = Arc::new(AtomicUsize::new(0));
    let update = phase.clone();
    let callback = Closure::<dyn FnMut(usize)>::new(move |index| {
        update.store(index, Ordering::Relaxed);
    });
    let (w, h) = session.dimensions();
    // An owned JS buffer can be transferred; WASM linear memory cannot.
    let rgb = Float32Array::from(session.rgb().as_slice());
    let worker = Worker(
        start(
            &serde_json::to_string(&models)?,
            rgb,
            w,
            h,
            callback.as_ref().unchecked_ref(),
        )
        .map_err(js_error)?,
    );
    let promise = Reflect::get(&worker.0, &"result".into())
        .map_err(js_error)?
        .dyn_into::<Promise>()
        .map_err(js_error)?;
    let result = wasm_bindgen_futures::JsFuture::from(promise).fuse();
    futures::pin_mut!(result);
    let output = loop {
        if cancelled.load(Ordering::Relaxed) {
            anyhow::bail!("cancelled");
        }
        let index = phase.load(Ordering::Relaxed);
        // Also checks that the workspace/job still exists, even during a long
        // synchronous prediction with no new progress messages from the worker.
        if !progress(
            schist_neural::BACKGROUND_REMOVAL_MODELS
                .get(index)
                .and_then(|id| schist_neural::spec(id))
                .map(|spec| spec.name),
        ) {
            anyhow::bail!("cancelled");
        }
        let tick = executor.timer(std::time::Duration::from_millis(100)).fuse();
        futures::pin_mut!(tick);
        select_biased! {
            output = result => break output.map_err(js_error)?,
            _ = tick => {},
        }
    };
    let floats = |name: &str, len: usize| -> anyhow::Result<Vec<f32>> {
        let array = Reflect::get(&output, &name.into())
            .map_err(js_error)?
            .dyn_into::<Float32Array>()
            .map_err(js_error)?;
        anyhow::ensure!(
            array.length() as usize == len,
            "invalid worker {name} length"
        );
        Ok(array.to_vec())
    };
    session
        .prepare_with_foreground(
            &floats("alpha", w * h)?,
            &floats("foreground", w * h * 3)?,
            duplicate_name,
        )
        .map_err(|error| anyhow::anyhow!("invalid generated mask: {error:?}"))
}

// Called only in the worker. Install still enforces the catalogue SHA-256.
#[wasm_bindgen]
pub fn background_removal_install(id: &str, bytes: &[u8]) -> Result<(), JsValue> {
    if !schist_neural::BACKGROUND_REMOVAL_MODELS.contains(&id) {
        return Err(JsValue::from_str("unknown background model"));
    }
    schist_neural::install(schist_neural::spec(id).unwrap(), bytes)
        .map(|_| ())
        .map_err(|error| JsValue::from_str(&format!("{error:#}")))
}

#[wasm_bindgen]
pub struct BackgroundRemovalOutput {
    alpha: Vec<f32>,
    foreground: Vec<f32>,
}

#[wasm_bindgen]
impl BackgroundRemovalOutput {
    pub fn take_alpha(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.alpha)
    }
    pub fn take_foreground(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.foreground)
    }
}

#[wasm_bindgen]
pub fn background_removal_infer(
    rgb: &[f32],
    width: usize,
    height: usize,
) -> Result<BackgroundRemovalOutput, JsValue> {
    let count = width
        .checked_mul(height)
        .filter(|&n| n > 0 && n <= 16_777_216);
    if count.and_then(|n| n.checked_mul(3)) != Some(rgb.len())
        || rgb
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(JsValue::from_str("invalid background-removal input"));
    }
    infer(rgb, width, height, &AtomicBool::new(false))
        .map(|(alpha, foreground)| BackgroundRemovalOutput { alpha, foreground })
        .map_err(|error| JsValue::from_str(&format!("{error:#}")))
}
