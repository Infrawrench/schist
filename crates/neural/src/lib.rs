//! Neural network inference: the Neural Filters, and the two tools that
//! need a model as much as any of them.
//!
//! Runs ONNX models through [`tract`], which is pure Rust -- no ONNX
//! Runtime, no C toolchain, nothing to install. That matters here: a paint
//! program that needs a 300 MB runtime and a matching CUDA to sharpen a
//! photo is not a paint program anyone will use.
//!
//! The installed effects backend runs compatible graphs on the GPU. On native
//! hosts, graphs with unsupported operators or large tensors also offload
//! individual convolutions and matrix contractions, with tract handling the
//! remaining operations and any device failure. See `docs/neural-gpu.md`.
//!
//! Model sources:
//!
//! * **Built in.** `detail.onnx`, `dejpeg.onnx`, `colorize.onnx`,
//!   `portrait.onnx`, `inpaint.onnx`, the waifu2x upscalers and
//!   `anti-smudge.onnx.xz` and the background-removal refiners/guide ship inside
//!   the binary. XZ weights stay compressed until first use; see the respective
//!   feature documents for provenance.
//! * **Downloaded.** The style-transfer, depth, face and segmentation
//!   networks are megabytes to tens of megabytes each and are somebody
//!   else's work, so they are fetched on demand into the user's data
//!   directory and checked against a known hash.
//! * **Generated.** The optional matting detector is exported locally from
//!   pinned upstream weights. Its checksum is verified when loaded; see
//!   `docs/background-removal.md`. There is no arbitrary-model import action.
//!
//! And two ways of feeding one, which is what [`Input`] distinguishes: a
//! model that *changes* an image sees it in tiles at full resolution,
//! while a model that answers a question *about* an image -- where the
//! faces are, what is near -- sees the whole thing resampled into one
//! fixed frame.
//!
//! Most filters that use a model also work without it, and so does
//! every tool. The classical implementation is not a stub -- it is the
//! fallback. Anti-Smudge leaves the image unchanged if its model cannot
//! run. The other fallbacks run when a model is
//! missing, fails, or looks at the picture and has nothing to say.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use anyhow::{bail, Context as _, Result};
use tract_onnx::prelude::*;

mod colour;
mod compat;
mod depth;
#[cfg(not(target_arch = "wasm32"))]
mod execution;
mod gpu;
mod gpu_image;
mod gpu_partition;
#[cfg(not(target_arch = "wasm32"))]
pub use execution::with_adaptive_execution;
// The gallery's search embeddings. Desktop only with the gallery — the
// tokenizer tables it carries would be dead weight in the wasm module.
#[cfg(not(target_arch = "wasm32"))]
pub mod embed;
mod face_rect;
mod faces;
pub use face_rect::{FaceRect, SAME_FACE_IOU};
#[cfg(target_os = "macos")]
mod accelerate_conv;
#[cfg(target_os = "macos")]
mod accelerate_matrix;
mod deform_sample;
mod detail_matting;
mod foreground_color;
mod framed;
mod gather_copy;
mod gather_nd;
mod halo;
mod inpaint;
mod matting;
mod pad_copy;
mod resample;
mod resize_copy;
mod segment;
mod subject_guidance;
mod tensor_layout;
#[cfg(target_os = "macos")]
mod vector_softmax;

// Development control for paired before/after timings in the same executable.
// No weights, tile geometry, backend placement or image processing settings vary.
pub(crate) fn fast_host_ops() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("SCHIST_NEURAL_LEGACY_HOST").is_none())
    }
    #[cfg(target_arch = "wasm32")]
    true
}

// Paired performance/accuracy diagnostics keep the original model graph.
pub(crate) fn fast_compute_ops() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("SCHIST_NEURAL_LEGACY_COMPUTE").is_none())
    }
    #[cfg(target_arch = "wasm32")]
    true
}

fn fast_model_ops() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("SCHIST_NEURAL_LEGACY_MODEL").is_none())
    }
    #[cfg(target_arch = "wasm32")]
    true
}
mod tile;
pub use colour::{chroma, recolour};
pub use depth::depth_map;
pub use faces::{embed_face, faces, Face, FACE_EMBED_DIM};
pub use foreground_color::{clean_foreground, clean_foreground_cancellable};
pub use framed::run_framed;
pub use inpaint::inpaint;
pub use matting::{refine_alpha, refine_alpha_cancellable};
pub use segment::{foreground, segment};
pub use subject_guidance::{guide_foreground, guide_foreground_with_reference};
pub use tile::{run_scaled, run_tiled, try_restore, try_run_tiled};

/// The models shipped inside the binary.
///
/// Not on the web: model payloads in the baseline download for filters that
/// may never run is the wrong trade there, so the same files are served
/// beside the app (`tools/web-build.sh` copies them) and fetched into the
/// in-memory store on demand, like any other download. The catalogue's
/// `bytes` fields are therefore literals rather than `.len()` of these;
/// `built_in_sizes_match_the_catalogue` (below) keeps them honest.
#[cfg(any(not(target_arch = "wasm32"), schist_library))]
const ANTI_SMUDGE_ONNX_XZ: &[u8] = include_bytes!("../models/anti-smudge.onnx.xz");
#[cfg(any(not(target_arch = "wasm32"), schist_library))]
const DETAIL_ONNX: &[u8] = include_bytes!("../models/detail.onnx");
#[cfg(any(not(target_arch = "wasm32"), schist_library))]
const DEJPEG_ONNX: &[u8] = include_bytes!("../models/dejpeg.onnx");
#[cfg(any(not(target_arch = "wasm32"), schist_library))]
const COLORIZE_ONNX: &[u8] = include_bytes!("../models/colorize.onnx");
#[cfg(any(not(target_arch = "wasm32"), schist_library))]
const PORTRAIT_ONNX: &[u8] = include_bytes!("../models/portrait.onnx");
#[cfg(any(not(target_arch = "wasm32"), schist_library))]
const INPAINT_ONNX: &[u8] = include_bytes!("../models/inpaint.onnx");
#[cfg(any(not(target_arch = "wasm32"), schist_library))]
const DETAIL_MATTING_ONNX_XZ: &[u8] = include_bytes!("../models/detail-matting.onnx.xz");
#[cfg(any(not(target_arch = "wasm32"), schist_library))]
const MATTING_ONNX_XZ: &[u8] = include_bytes!("../models/matting.onnx.xz");
#[cfg(any(not(target_arch = "wasm32"), schist_library))]
const SUBJECT_GUIDE_ONNX_XZ: &[u8] = include_bytes!("../models/subject-guide.onnx.xz");
#[cfg(any(not(target_arch = "wasm32"), schist_library))]
const WAIFU2X_ART_ONNX: &[u8] = include_bytes!("../models/waifu2x-art.onnx");
#[cfg(any(not(target_arch = "wasm32"), schist_library))]
const WAIFU2X_PHOTO_ONNX: &[u8] = include_bytes!("../models/waifu2x-photo.onnx");

/// How a model wants its pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Range {
    /// Channels in 0..=1, which is how everything here stores pixels.
    Unit,
    /// Channels in 0..=255, the convention the torchvision-derived
    /// style-transfer networks were trained with.
    Byte,
    /// `(v - mean) / sd` per channel, on channels in 0..=1: the
    /// ImageNet normalisation every torchvision backbone was fitted
    /// with, and which its successors kept.
    Standard { mean: [f32; 3], sd: [f32; 3] },
}

impl Range {
    /// A 0..=1 channel value as the model wants it.
    fn encode(self, v: f32, c: usize) -> f32 {
        match self {
            Range::Unit => v,
            Range::Byte => v * 255.0,
            Range::Standard { mean, sd } => (v - mean[c]) / sd[c],
        }
    }

    /// The inverse, for a model whose output is an image again.
    fn decode(self, v: f32, c: usize) -> f32 {
        match self {
            Range::Unit => v,
            Range::Byte => v / 255.0,
            Range::Standard { mean, sd } => v * sd[c] + mean[c],
        }
    }
}

/// The ImageNet statistics, spelled once.
const IMAGENET: Range = Range::Standard {
    mean: [0.485, 0.456, 0.406],
    sd: [0.229, 0.224, 0.225],
};

/// What to do when the image and the model's frame are not the same
/// shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Squash it. Fine for a model whose answer is per-pixel and whose
    /// subject is the whole scene.
    Stretch,
    /// Letterbox it, so nothing is distorted. What a detector wants:
    /// squash a 16:9 panorama into a 4:3 frame and the faces in it stop
    /// looking like faces.
    Contain,
}

/// How a model wants its input framed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// Square tiles cut from the image at full resolution, stitched back
    /// with `overlap` pixels of context trimmed off each edge. What an
    /// image-to-image model wants: it works on pixels, and there are
    /// however many of those there are. `scale` is how many output pixels
    /// the model makes of each input one -- 1 for a filter, more for an
    /// upscaler.
    Tiles {
        size: usize,
        overlap: usize,
        scale: usize,
    },
    /// One fixed frame the whole image is resampled into. What a model
    /// that answers a question *about* the picture wants -- where the
    /// faces are, how far away things are -- because that answer needs
    /// all of the picture and needs none of its resolution.
    Frame {
        width: usize,
        height: usize,
        fit: Fit,
    },
    /// A fixed run of token ids — no pixels at all. What the text half
    /// of a dual-encoder wants; `context` is its sequence length.
    Tokens { context: usize },
}

impl Input {
    /// The (width, height) the graph is fixed to.
    pub fn dims(self) -> (usize, usize) {
        match self {
            Input::Tiles { size, .. } => (size, size),
            Input::Frame { width, height, .. } => (width, height),
            Input::Tokens { context } => (context, 1),
        }
    }

    /// Output pixels per input pixel.
    fn scale(self) -> usize {
        match self {
            Input::Tiles { scale, .. } => scale,
            Input::Frame { .. } | Input::Tokens { .. } => 1,
        }
    }
}

/// Where a model comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    BuiltIn,
    Download(&'static str),
    /// Reproducible local export; no arbitrary-model import UI or network URL.
    Generated,
}

/// A model this build knows about.
#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// File name inside the model directory.
    pub file: &'static str,
    pub source: ModelSource,
    /// SHA-256 of the file, so a truncated or substituted download is
    /// rejected rather than run.
    pub sha256: Option<&'static str>,
    pub bytes: usize,
    /// How the image is presented to the graph.
    pub input: Input,
    pub range: Range,
    pub license: &'static str,
    pub note: &'static str,
}

impl ModelSpec {
    pub fn built_in(&self) -> bool {
        self.source == ModelSource::BuiltIn
    }
}

/// Every model the Neural Filters can use.
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "anti-smudge",
        name: "Anti-Smudge",
        file: "anti-smudge.onnx.xz",
        source: ModelSource::BuiltIn,
        sha256: Some("e8a9fa635ecf5bdbedad4944ad7a776d36b12d658dca7e994f73bc408cd2a875"),
        bytes: 23_504_872, // Compressed artifact size.
        input: Input::Tiles { size: 2048, overlap: 96, scale: 1 },
        range: Range::Unit,
        license: "MFDNet / FlareReal600",
        note: "", // Provenance and upstream terms are in docs/anti-smudge.md.
    },
    ModelSpec {
        id: "detail-matting",
        name: "ViTMatte-S",
        file: "detail-matting.onnx.xz",
        source: ModelSource::BuiltIn,
        sha256: Some("3c31c66e8ca3a9ec550fec06e2b23c7ac57225fe7a97953e4d8ae38c326a6f37"),
        bytes: 95_638_600,
        input: Input::Tiles { size: 768, overlap: 128, scale: 1 },
        range: Range::Unit,
        license: "ViTMatte MIT; Hugging Face weights Apache-2.0",
        note: "tools/train/export_detail_matting.py; docs/background-removal.md",
    },
    ModelSpec {
        id: "foreground-matting",
        name: "BiRefNet Lite Matting",
        file: "foreground-birefnet-matting.onnx",
        source: ModelSource::Generated,
        sha256: Some("273501048979b3012b544232234618819225745a13e316b21b4db439a2f28fe8"),
        bytes: 223_559_431,
        input: Input::Frame { width: 1024, height: 1024, fit: Fit::Stretch },
        range: IMAGENET,
        license: "BiRefNet, Zheng Peng et al., MIT",
        note: "tools/train/export_foreground.py; docs/background-removal.md",
    },
    ModelSpec {
        id: "subject-guide",
        name: "DeepLabV3 Subject Guide",
        file: "subject-guide.onnx.xz",
        source: ModelSource::BuiltIn,
        sha256: Some("883dd1d4b4c7bdfc24c38ba516f2a299fa0fe1af355f22af4906a1627daa07fa"),
        bytes: 40_454_876,
        input: Input::Frame { width: 520, height: 520, fit: Fit::Stretch },
        range: IMAGENET,
        license: "Torchvision, BSD-3-Clause; pretrained COCO/VOC weights",
        note: "tools/train/export_subject_guide.py; docs/background-removal.md",
    },
    ModelSpec {
        id: "matting",
        name: "Schist MatteNet",
        file: "matting.onnx.xz",
        source: ModelSource::BuiltIn,
        sha256: Some("485a9ba783fde2442aa00f52a90a7aa027e086745045f99a16b4ddf4f9b60a2c"),
        bytes: 81_960,
        input: Input::Tiles { size: 128, overlap: 16, scale: 1 },
        range: Range::Unit,
        license: "Schist; MicroMat-3K (CC BY 4.0)",
        note: "tools/train/matting.py; docs/background-removal.md",
    },
    ModelSpec {
        id: "foreground",
        name: "BiRefNet Lite",
        file: "foreground-birefnet-lite.onnx",
        source: ModelSource::Download("https://github.com/ZhengPeng7/BiRefNet/releases/download/v1/BiRefNet-general-bb_swin_v1_tiny-epoch_232.onnx"),
        sha256: Some("5600024376f572a557870a5eb0afb1e5961636bef4e1e22132025467d0f03333"),
        bytes: 224_005_088,
        input: Input::Frame { width: 1024, height: 1024, fit: Fit::Stretch },
        range: Range::Standard { mean: [0.485, 0.456, 0.406], sd: [0.229, 0.224, 0.225] },
        license: "BiRefNet, Zheng Peng et al., MIT",
        note: "https://github.com/ZhengPeng7/BiRefNet",
    },
    ModelSpec {
        id: "detail",
        name: "Detail (Super Zoom)",
        file: "detail.onnx",
        source: ModelSource::BuiltIn,
        sha256: None,
        bytes: 156_906,
        input: Input::Tiles {
            size: 128,
            overlap: 8,
            scale: 1,
        },
        range: Range::Unit,
        license: "Trained for Schist; same licence as the app",
        note: "Restores the high frequencies an enlargement loses. Trained \
               on the Kodak image suite; see tools/train/detail.py.",
    },
    ModelSpec {
        id: "dejpeg",
        name: "Deblock (JPEG Artifact Removal)",
        file: "dejpeg.onnx",
        source: ModelSource::BuiltIn,
        sha256: None,
        bytes: 261_562,
        input: Input::Tiles {
            size: 128,
            overlap: 8,
            scale: 1,
        },
        range: Range::Unit,
        license: "Trained for Schist; same licence as the app",
        note: "Removes the blocking and ringing JPEG leaves behind. Trained \
               on the Kodak image suite compressed at every quality from 10 \
               to 60; see tools/train/dejpeg.py.",
    },
    ModelSpec {
        id: "colorize",
        name: "Colour (Colorize)",
        file: "colorize.onnx",
        source: ModelSource::BuiltIn,
        sha256: None,
        bytes: 1_864_870,
        // Chroma is low-frequency and colour has to agree across a whole
        // subject, so this one sees the picture whole and small rather
        // than sharp and in pieces.
        input: Input::Frame {
            width: 256,
            height: 256,
            fit: Fit::Stretch,
        },
        range: Range::Unit,
        license: "Trained for Schist; same licence as the app",
        note: "Predicts colour for a greyscale photograph. Trained on \
               20,000 CC BY photographs from Open Images; see \
               tools/train/colorize.py.",
    },
    ModelSpec {
        id: "portrait",
        name: "Portrait (Sketch to Portrait)",
        file: "portrait.onnx",
        source: ModelSource::BuiltIn,
        sha256: None,
        bytes: 1_795_756,
        // A face at a time, whole: filling in a drawing means knowing
        // what the drawing is of, and no tile of a face knows that.
        input: Input::Frame {
            width: 128,
            height: 128,
            fit: Fit::Stretch,
        },
        range: Range::Unit,
        license: "Trained for Schist; same licence as the app",
        note: "Puts the tone and colour back into a sketch of a face. \
               Trained to invert this build's own Photo to Sketch on CC BY \
               faces from Open Images; see tools/train/portrait.py.",
    },
    ModelSpec {
        id: "inpaint",
        name: "Fill (Content-Aware Fill)",
        file: "inpaint.onnx",
        source: ModelSource::BuiltIn,
        sha256: None,
        bytes: 2_894_365,
        // The region whole, because the answer for a pixel in the middle
        // of a hole is not anywhere near it -- there is nothing near it --
        // it is at the far side, and no tile of a hole can see that.
        input: Input::Frame {
            width: 160,
            height: 160,
            fit: Fit::Stretch,
        },
        range: Range::Unit,
        license: "Trained for Schist; same licence as the app",
        note: "Predicts what was behind a hole, from the picture round \
               it. Trained on CC BY photographs from Open Images; see \
               tools/train/inpaint.py.",
    },
    ModelSpec {
        id: "waifu2x-art",
        name: "waifu2x ×2 (Art)",
        file: "waifu2x-art.onnx",
        source: ModelSource::BuiltIn,
        sha256: None,
        bytes: 2_213_982,
        input: Input::Tiles {
            size: 128,
            overlap: 8,
            scale: 2,
        },
        range: Range::Unit,
        note: "Doubles an image's size, drawn edges staying edges. The \
               upconv_7 art model from the waifu2x project, trained on \
               illustrations; see tools/train/waifu2x.py.",
        license: "waifu2x (nagadomi), MIT",
    },
    ModelSpec {
        id: "waifu2x-photo",
        name: "waifu2x ×2 (Photo)",
        file: "waifu2x-photo.onnx",
        source: ModelSource::BuiltIn,
        sha256: None,
        bytes: 2_213_982,
        input: Input::Tiles {
            size: 128,
            overlap: 8,
            scale: 2,
        },
        range: Range::Unit,
        note: "Doubles an image's size. The upconv_7 photo model from the \
               waifu2x project, trained on photographs; see \
               tools/train/waifu2x.py.",
        license: "waifu2x (nagadomi), MIT",
    },
    ModelSpec {
        id: "style-mosaic",
        name: "Style: Mosaic",
        file: "style-mosaic.onnx",
        source: ModelSource::Download("https://github.com/onnx/models/raw/main/validated/vision/style_transfer/fast_neural_style/model/mosaic-9.onnx"),
        sha256: Some("fa646dedade881243f8d5a2ceb7de2b93675b21fc24f7482894ac4851a9a0a47"),
        bytes: 6_728_029,
        input: Input::Tiles { size: 384, overlap: 32, scale: 1 },
        range: Range::Byte,
        license: "ONNX Model Zoo, Apache-2.0",
        note: "Fast neural style transfer (Johnson et al.).",
    },
    ModelSpec {
        id: "style-candy",
        name: "Style: Candy",
        file: "style-candy.onnx",
        source: ModelSource::Download("https://github.com/onnx/models/raw/main/validated/vision/style_transfer/fast_neural_style/model/candy-9.onnx"),
        sha256: Some("9d11a3529d1e547da6ae07201d93484dbab2ec0a3614535752c8f40f0fe2968a"),
        bytes: 6_728_029,
        input: Input::Tiles { size: 384, overlap: 32, scale: 1 },
        range: Range::Byte,
        license: "ONNX Model Zoo, Apache-2.0",
        note: "Fast neural style transfer (Johnson et al.).",
    },
    ModelSpec {
        id: "style-udnie",
        name: "Style: Udnie",
        file: "style-udnie.onnx",
        source: ModelSource::Download("https://github.com/onnx/models/raw/main/validated/vision/style_transfer/fast_neural_style/model/udnie-9.onnx"),
        sha256: Some("8656b6ce7dec8f22ee13c2d557d6b67bd6f550dde88d0f2e7c9972aeb765cc0d"),
        bytes: 6_728_029,
        input: Input::Tiles { size: 384, overlap: 32, scale: 1 },
        range: Range::Byte,
        license: "ONNX Model Zoo, Apache-2.0",
        note: "Fast neural style transfer (Johnson et al.).",
    },
    ModelSpec {
        id: "depth",
        name: "Depth (Depth Blur)",
        file: "depth.onnx",
        source: ModelSource::Download("https://github.com/isl-org/MiDaS/releases/download/v2_1/model-small.onnx"),
        sha256: Some("2d8c6cb8f415229daf1eb041024208e2608c9f98e17c81cc7c6ecb449c56fd58"),
        bytes: 66_764_249,
        input: Input::Frame { width: 256, height: 256, fit: Fit::Stretch },
        range: IMAGENET,
        license: "MiDaS v2.1 small, Intel ISL, MIT",
        note: "Estimates how far away everything in a photograph is, from \
               the photograph alone (Ranftl et al.).",
    },
    ModelSpec {
        id: "segment",
        name: "Objects (Object Selection)",
        file: "segment.onnx",
        source: ModelSource::Download("https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2netp.onnx"),
        sha256: Some("309c8469258dda742793dce0ebea8e6dd393174f89934733ecc8b14c76f4ddd8"),
        bytes: 4_574_861,
        input: Input::Frame { width: 320, height: 320, fit: Fit::Stretch },
        range: IMAGENET,
        license: "U^2-Net, Qin et al., Apache-2.0",
        note: "Separates the subject of a picture from its background, so \
               Object Selection can cut round an object instead of round \
               everything that is not the colour behind it.",
    },
    ModelSpec {
        id: "face",
        name: "Faces (Detection)",
        file: "face.onnx",
        source: ModelSource::Download("https://github.com/onnx/models/raw/main/validated/vision/body_analysis/ultraface/models/version-RFB-320.onnx"),
        sha256: Some("34cd7e60aeff28744c657de7a3dc64e872d506741de66987f3426f2b79f88017"),
        bytes: 1_270_727,
        input: Input::Frame { width: 320, height: 240, fit: Fit::Contain },
        range: Range::Standard { mean: [0.498_039_2; 3], sd: [0.501_960_8; 3] },
        license: "ONNX Model Zoo, MIT",
        note: "Finds faces, so Skin Smoothing can work on skin that is on \
               one rather than on anything skin-coloured — and so the \
               gallery can find the people in a photo.",
    },
    // Who a face belongs to: SFace (a MobileFaceNet trained with the
    // SFace loss, from the OpenCV Zoo) maps a 112x112 face crop to 128
    // numbers whose cosine says whether two crops are the same person.
    // The gallery's People feature compares every detected face against
    // the faces already named, and suggests. OpenCV feeds it BGR in
    // 0..=255; the caller swaps the channels, the range does the scale.
    ModelSpec {
        id: "face-embed",
        name: "Faces (Recognition)",
        file: "face-embed.onnx",
        source: ModelSource::Download("https://github.com/opencv/opencv_zoo/raw/47534e27c9851bb1128ccc0102f1145e27f23f98/models/face_recognition_sface/face_recognition_sface_2021dec.onnx"),
        sha256: Some("0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79"),
        bytes: 38_696_353,
        input: Input::Frame { width: 112, height: 112, fit: Fit::Stretch },
        range: Range::Byte,
        license: "OpenCV Zoo SFace (Zhong & Deng), Apache-2.0",
        note: "Tells faces apart, so the gallery can suggest who a face \
               is once you have named them elsewhere. Runs on each face \
               the detector finds, in the background.",
    },
    ModelSpec {
        id: "nsfw",
        name: "Content (NSFW Filter)",
        file: "nsfw.onnx",
        // Revision-pinned so the hash below stays true whatever happens
        // to the repository's main branch.
        source: ModelSource::Download("https://huggingface.co/Sunxyw/nsfwjs-onnx/resolve/38708a81164d44faab3e6fd4c9f2543db5ddf473/onnx/model_quantized.onnx"),
        sha256: Some("547891d566e735260b312fd865c40228e0ea75a1d1dd913653555c94e2ad49fd"),
        bytes: 17_320_391,
        input: Input::Frame { width: 224, height: 224, fit: Fit::Stretch },
        range: Range::Unit,
        license: "NSFWJS model (Infinite Red / GantMan), MIT",
        note: "Says how likely a photograph is to be explicit — five \
               softmax classes (drawing, hentai, neutral, porn, sexy) — \
               so the gallery can hide what the content-filter \
               preference asks it to.",
    },
    // The two halves of the gallery's search: pictures and words mapped
    // into the same 512 dimensions, so "dog on a beach" lands near the
    // photographs of one. MobileCLIP-S0 because its image tower is
    // convolutional — a ViT this size took forty times longer under
    // tract — and the text tower's bulk is its embedding table, so a
    // query costs milliseconds.
    ModelSpec {
        id: "embed-image",
        name: "Search (Image Embeddings)",
        file: "embed-image.onnx",
        source: ModelSource::Download("https://huggingface.co/Xenova/mobileclip_s0/resolve/757d59c9c6870a76a4b0306f05f5061bca15c39f/onnx/vision_model.onnx"),
        sha256: Some("17d3c037b1d488c10c50e09f6009ea5a198caef4e0e8f4ea5617b7cb2d067ac0"),
        bytes: 45_543_630,
        input: Input::Frame { width: 256, height: 256, fit: Fit::Stretch },
        range: Range::Unit,
        license: "MobileCLIP-S0 weights (Apple ML research licence); ONNX export by Xenova",
        note: "Turns a photograph into the coordinates the gallery's \
               search ranks against. Runs once per photo, on its \
               thumbnail, in the background.",
    },
    ModelSpec {
        id: "embed-text",
        name: "Search (Text Embeddings)",
        file: "embed-text.onnx",
        source: ModelSource::Download("https://huggingface.co/Xenova/mobileclip_s0/resolve/757d59c9c6870a76a4b0306f05f5061bca15c39f/onnx/text_model.onnx"),
        sha256: Some("f6e9bd5742bfc515889e901634d8a2ff2a57fab8564e4ad3760e800b1a51b77c"),
        bytes: 169_807_789,
        input: Input::Tokens { context: 77 },
        range: Range::Unit,
        license: "MobileCLIP-S0 weights (Apple ML research licence); ONNX export by Xenova",
        note: "Turns a search query into the same coordinates the \
               photographs live in. Most of its weight is the \
               vocabulary table; a query runs in milliseconds.",
    },
];

pub fn spec(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|m| m.id == id)
}

/// Prefer the matting-trained weights after their reproducible local export.
/// The downloadable general detector remains available on other installations.
pub fn foreground_model_id() -> &'static str {
    if installed("foreground-matting") && installed("foreground") {
        "foreground-matting"
    } else {
        "foreground"
    }
}

/// Prefer the bundled native-resolution trimap refiner when available.
pub fn matting_model_id() -> &'static str {
    if installed("detail-matting") {
        "detail-matting"
    } else {
        "matting"
    }
}

/// Where a build that lacks a model can fetch it, or `None` when it
/// cannot.
///
/// Natively that is the catalogue URL (built-ins need no fetching). On the
/// web it is the other way round: the formerly-embedded models are served
/// beside the app and fetched same-origin, while the external ones are
/// unreachable — their GitHub URLs redirect through a host that sends no
/// CORS headers, so a browser fetch is refused before it starts.
pub fn download_url(spec: &ModelSpec) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        spec.built_in()
            .then(|| format!("assets/models/{}", spec.file))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match spec.source {
            ModelSource::Download(url) => Some(url.to_owned()),
            ModelSource::BuiltIn | ModelSource::Generated => None,
        }
    }
}

/// The web build's model store: fetched bytes, held in memory for the
/// life of the tab. A browser offers no directory to write into, and the
/// re-fetching per session — usually straight from the HTTP cache — avoids
/// persistent storage. Compressed models stay compressed in this store.
#[cfg(target_arch = "wasm32")]
fn web_store() -> &'static RwLock<HashMap<&'static str, Vec<u8>>> {
    static STORE: OnceLock<RwLock<HashMap<&'static str, Vec<u8>>>> = OnceLock::new();
    STORE.get_or_init(Default::default)
}

/// Where downloaded models live.
pub fn model_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SCHIST_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".local/share")
        });
    base.join("schist/models")
}

/// Whether a model is ready to run.
#[cfg(not(schist_library))]
pub fn installed(id: &str) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        spec(id).is_some_and(|s| web_store().read().is_ok_and(|m| m.contains_key(s.file)))
    }
    #[cfg(not(target_arch = "wasm32"))]
    match spec(id) {
        Some(s) if s.built_in() => true,
        Some(s) => model_dir().join(s.file).exists(),
        None => false,
    }
}

/// A loaded model, ready to run.
pub struct Model {
    plan: Arc<TypedSimplePlan>,
    gpu: Option<gpu::Network>,
    partitioned: Option<gpu_partition::Partitioned>,
    /// Planes the graph's input takes, which is three for everything
    /// that sees only colour.
    channels: usize,
    /// The graph wants channels-last input (NHWC), the TensorFlow way.
    /// Models converted from TF keep that layout; everything trained
    /// for Schist is channels-first.
    nhwc: bool,
    /// Optional working resolution for models that predict broad lens scatter.
    restoration_max_side: Option<usize>,
    restoration_tile_size: Option<usize>,
    restoration_halo_cleanup: bool,
    pub spec: &'static ModelSpec,
}

/// The fixed dimensions an ONNX graph declares on its first input, in
/// order, with anything symbolic or absent as `None`.
fn declared_dims(proto: &tract_onnx::pb::ModelProto) -> Vec<Option<usize>> {
    let Some(dim) = proto
        .graph
        .as_ref()
        .and_then(|g| g.input.first())
        .and_then(|i| i.r#type.as_ref())
        .and_then(|t| t.value.as_ref())
    else {
        return Vec::new();
    };
    let tract_onnx::pb::type_proto::Value::TensorType(t) = dim;
    let Some(shape) = t.shape.as_ref() else {
        return Vec::new();
    };
    shape
        .dim
        .iter()
        .map(|d| match d.value.as_ref() {
            Some(tract_onnx::pb::tensor_shape_proto::dimension::Value::DimValue(v)) if *v > 0 => {
                Some(*v as usize)
            }
            _ => None,
        })
        .collect()
}

/// The channel count an ONNX graph declares on its first input, when it
/// declares a fixed one.
fn declared_channels(proto: &tract_onnx::pb::ModelProto) -> Option<usize> {
    declared_dims(proto).get(1).copied().flatten()
}

/// Whether the first input is declared channels-last: rank four with
/// three at the end and not at the channel-first position.
fn declared_nhwc(proto: &tract_onnx::pb::ModelProto) -> bool {
    let dims = declared_dims(proto);
    dims.len() == 4 && dims[3] == Some(3) && dims[1] != Some(3)
}

impl Model {
    /// Load from ONNX bytes, fixing the input to one frame so tract can
    /// optimize the graph completely rather than for an unknown size.
    /// XZ-compressed ONNX is expanded in memory for parsing.
    pub fn from_bytes(spec: &'static ModelSpec, bytes: &[u8]) -> Result<Model> {
        let bytes = decode_model_bytes(bytes)?;
        let (mut w, mut h) = spec.input.dims();
        let mut cursor = std::io::Cursor::new(bytes.as_ref());
        let mut onnx = tract_onnx::onnx();
        gather_nd::register(&mut onnx);
        deform_sample::register(&mut onnx);
        let mut proto = onnx
            .proto_model_for_read(&mut cursor)
            .context("not a readable ONNX model")?;
        if fast_host_ops() && matches!(spec.id, "foreground" | "foreground-matting") {
            gather_nd::specialize_pixel_indices(&mut proto);
        }
        if fast_model_ops() && matches!(spec.id, "foreground" | "foreground-matting") {
            let fused = deform_sample::optimize(&mut proto);
            log::info!(target: "schist_neural::execution", "{}: fused {fused} deformable samplers", spec.id);
        }
        let restoration_tile_size = if spec.id == "anti-smudge" {
            proto
                .metadata_props
                .iter()
                .find(|p| p.key == "schist.tile")
                .map(|p| p.value.parse::<usize>())
                .transpose()
                .context("invalid restoration tile size")?
        } else {
            None
        };
        if let Some(size) = restoration_tile_size {
            anyhow::ensure!(
                (32..=2048).contains(&size) && size % 32 == 0,
                "restoration tile must be a multiple of 32 in 32..=2048"
            );
            (w, h) = (size, size);
        }
        if compat::modernise(&mut proto) {
            log::debug!("{}: rewrote a pre-opset-10 graph", spec.id);
        }
        // A text tower takes token ids, not pixels: one integer row.
        if let Input::Tokens { context } = spec.input {
            if let Some(graph) = proto.graph.as_mut() {
                graph.value_info.clear();
            }
            let typed = onnx
                .model_for_proto_model(&proto)
                .context("not a model tract can parse")?
                .with_input_fact(0, i64::fact([1, context]).into())
                .context("model does not take a 1xN token input")?
                .into_typed()
                .context("model uses an operator tract cannot run")?;
            let plan = typed.clone().into_optimized()?.into_runnable()?;
            let partitioned = gpu_partition::Partitioned::compile(typed);
            return Ok(Model {
                plan,
                gpu: None,
                partitioned,
                channels: 0,
                nhwc: false,
                restoration_max_side: None,
                restoration_tile_size: None,
                restoration_halo_cleanup: false,
                spec,
            });
        }
        // Almost every vision model takes three planes of colour, but an
        // inpainting one takes four -- the fourth says which pixels are
        // missing, and it has to be a channel rather than a convention
        // because "black" and "gone" are otherwise the same pixel. Ask
        // the graph rather than assuming. TensorFlow conversions keep
        // channels-last instead; ask about that too.
        let nhwc = declared_nhwc(&proto);
        let restoration_max_side = if spec.id == "anti-smudge" {
            proto
                .metadata_props
                .iter()
                .find(|p| p.key == "schist.restore_max_side")
                .map(|p| p.value.parse::<usize>())
                .transpose()
                .context("invalid restoration working resolution")?
        } else {
            None
        };
        anyhow::ensure!(
            restoration_max_side.is_none_or(|side| (32..=2048).contains(&side)),
            "restoration working resolution must be 32..=2048"
        );
        let restoration_halo_cleanup = if spec.id == "anti-smudge" {
            match proto
                .metadata_props
                .iter()
                .find(|p| p.key == "schist.halo_cleanup")
            {
                Some(p) => {
                    anyhow::ensure!(
                        p.value == "radial-v1",
                        "unsupported restoration halo cleanup"
                    );
                    true
                }
                None => false,
            }
        } else {
            false
        };
        let channels = if nhwc {
            3
        } else {
            declared_channels(&proto).unwrap_or(3)
        };
        // Exporters leave shape hints on intermediate values, often with
        // symbolic batch/height/width; those fight the concrete input
        // fact below, and tract re-infers everything anyway.
        if let Some(graph) = proto.graph.as_mut() {
            graph.value_info.clear();
        }
        let fact = if nhwc {
            f32::fact([1, h, w, channels])
        } else {
            f32::fact([1, channels, h, w])
        };
        let mut inferred = onnx
            .model_for_proto_model(&proto)
            .context("not a model tract can parse")?
            .with_input_fact(0, fact.into())
            .context("model does not take the declared float input")?;
        inferred
            .analyse(false)
            .context("model uses an operator tract cannot infer")?;
        let gpu = gpu::Network::compile(
            &proto,
            if nhwc {
                vec![1, h, w, channels]
            } else {
                vec![1, channels, h, w]
            },
            &inferred,
        );
        let typed = inferred
            .into_typed()
            .context("model uses an operator tract cannot run")?;
        let has_batched_gather = proto.graph.as_ref().is_some_and(|g| {
            g.node.iter().any(|n| {
                n.op_type == "SchistDeformSample"
                    || (n.op_type == "GatherND"
                        && n.attribute
                            .iter()
                            .any(|a| a.name == "batch_dims" && a.i > 0))
            })
        });
        // tract's preliminary PushSliceUp rewrite panics on BiRefNet's
        // batched index tensors. Codegen optimization handles this graph.
        let mut cpu = typed.clone();
        if !has_batched_gather {
            cpu.declutter()?;
        }
        #[cfg(target_os = "macos")]
        if spec.id == "detail-matting"
            && fast_compute_ops()
            && std::env::var_os("SCHIST_NEURAL_LEGACY_CONV").is_none()
        {
            let count = accelerate_conv::optimize(&mut cpu)?;
            log::info!(target: "schist_neural::execution", "{}: accelerated {count} decoder convolutions", spec.id);
        }
        #[cfg(target_os = "macos")]
        if matches!(
            spec.id,
            "detail-matting" | "foreground" | "foreground-matting"
        ) && fast_compute_ops()
        {
            let count = accelerate_matrix::optimize(&mut cpu)?;
            log::info!(target: "schist_neural::execution", "{}: accelerated {count} matrix products", spec.id);
        }
        cpu.optimize()?;
        tensor_layout::optimize(&mut cpu)?;
        if fast_compute_ops() {
            pad_copy::optimize(&mut cpu)?;
        }
        if fast_host_ops() {
            gather_copy::optimize(&mut cpu)?;
            resize_copy::optimize(&mut cpu)?;
        }
        #[cfg(target_os = "macos")]
        if fast_model_ops()
            && matches!(
                spec.id,
                "foreground" | "foreground-matting" | "detail-matting" | "subject-guide"
            )
        {
            vector_softmax::optimize(&mut cpu)?;
        }
        let plan = cpu.into_runnable()?;
        let partitioned = if gpu.is_none() {
            if has_batched_gather {
                gpu_partition::Partitioned::compile_without_declutter(typed)
            } else {
                gpu_partition::Partitioned::compile(typed)
            }
        } else {
            None
        };
        Ok(Model {
            plan,
            gpu,
            partitioned,
            channels,
            nhwc,
            restoration_max_side,
            restoration_tile_size,
            restoration_halo_cleanup,
            spec,
        })
    }

    fn input_dims(&self) -> (usize, usize) {
        self.restoration_tile_size
            .map(|size| (size, size))
            .unwrap_or_else(|| self.spec.input.dims())
    }

    /// The checked resident graph, also submit-able by an asynchronous GPU host.
    /// Input uses the model's declared tensor layout and encoded value range.
    pub fn gpu_program(&self) -> Option<&schist_fx::ComputeProgram> {
        self.gpu.as_ref().map(|g| &g.program)
    }

    /// Number of GPU contractions available when the graph cannot run entirely
    /// on-device. Shape/integer operations stay on tract; oversized convolutions
    /// execute in exact bands without changing the model's input resolution.
    pub fn gpu_partition_count(&self) -> usize {
        self.partitioned.as_ref().map_or(0, |p| p.operations)
    }

    fn run_input(&self, inputs: TVec<TValue>) -> Result<TVec<TValue>> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(key) = execution::Key::for_model(self.spec.id, inputs[0].shape()) {
            let backend = schist_fx::backend();
            if backend.compute_available(usize::MAX) {
                return execution::run(
                    key,
                    backend,
                    || self.run_cpu(inputs.clone()),
                    || self.run_accelerated(inputs.clone()),
                );
            }
        }
        self.run_accelerated(inputs)
    }

    fn run_accelerated(&self, inputs: TVec<TValue>) -> Result<TVec<TValue>> {
        if let Some(gpu) = &self.gpu {
            if let Ok(view) = inputs[0].to_plain_array_view::<f32>() {
                if let Some(output) = view
                    .as_slice()
                    .and_then(|input| schist_fx::try_compute(input, &gpu.program))
                {
                    let mut results = tvec!();
                    let mut start = 0;
                    for shape in &gpu.shapes {
                        let len: usize = shape.iter().product();
                        results
                            .push(Tensor::from_shape(shape, &output[start..start + len])?.into());
                        start += len;
                    }
                    return Ok(results);
                }
            }
        }
        if schist_fx::backend().compute_available(usize::MAX) {
            if let Some(partitioned) = &self.partitioned {
                return partitioned.plan.run(inputs);
            }
        }
        self.run_cpu(inputs)
    }

    fn run_cpu(&self, inputs: TVec<TValue>) -> Result<TVec<TValue>> {
        #[cfg(not(target_arch = "wasm32"))]
        return execution::run_cpu(self.spec.id, &self.plan, inputs);
        #[cfg(target_arch = "wasm32")]
        self.plan.run(inputs)
    }

    /// How many planes the graph wants.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Run the graph on planes the caller has already separated, for a
    /// model whose input is not simply three of colour.
    pub(crate) fn run_planes(&self, planes: &[&[f32]]) -> Result<TVec<TValue>> {
        let (w, h) = self.input_dims();
        if planes.len() != self.channels || planes.iter().any(|p| p.len() != w * h) {
            bail!(
                "expected {} planes of {} floats, got {:?}",
                self.channels,
                w * h,
                planes.iter().map(|p| p.len()).collect::<Vec<_>>()
            );
        }
        let input = tract_ndarray::Array4::<f32>::from_shape_fn(
            (1, self.channels, h, w),
            |(_, c, y, x)| planes[c][y * w + x],
        );
        self.run_input(tvec!(input.into_tensor().into()))
    }

    /// Run the graph over one frame of interleaved RGB in 0..=1, sized
    /// exactly as the spec says, and hand back its outputs untouched.
    fn run(&self, rgb: &[f32]) -> Result<TVec<TValue>> {
        let (w, h) = self.input_dims();
        if rgb.len() != w * h * 3 {
            bail!("expected {} floats, got {}", w * h * 3, rgb.len());
        }
        let range = self.spec.range;
        // Interleaved RGB to whichever layout the graph wants: planar
        // NCHW for most ONNX vision models, channels-last for the ones
        // that came from TensorFlow.
        let input = if self.nhwc {
            tract_ndarray::Array4::<f32>::from_shape_fn((1, h, w, 3), |(_, y, x, c)| {
                range.encode(rgb[(y * w + x) * 3 + c], c)
            })
        } else {
            tract_ndarray::Array4::<f32>::from_shape_fn((1, 3, h, w), |(_, c, y, x)| {
                range.encode(rgb[(y * w + x) * 3 + c], c)
            })
        };
        self.run_input(tvec!(input.into_tensor().into()))
    }

    /// Run a *classifier*: one frame of interleaved RGB in 0..=1, sized
    /// as the spec says, and the first output flattened to plain floats
    /// — softmax scores or logits, whatever the graph emits.
    pub fn run_scores(&self, rgb: &[f32]) -> Result<Vec<f32>> {
        let out = self.run(rgb)?;
        let view = out[0].to_plain_array_view::<f32>()?;
        Ok(view.iter().copied().collect())
    }

    /// Run a token-input model (a text encoder) over one padded row of
    /// ids and hand back the first output flattened.
    pub fn run_token_scores(&self, ids: &[i64]) -> Result<Vec<f32>> {
        let (context, _) = self.spec.input.dims();
        if ids.len() != context {
            bail!("expected {context} token ids, got {}", ids.len());
        }
        let input = tract_ndarray::Array2::<i64>::from_shape_vec((1, context), ids.to_vec())?;
        let out = self.run_input(tvec!(input.into_tensor().into()))?;
        let view = out[0].to_plain_array_view::<f32>()?;
        Ok(view.iter().copied().collect())
    }

    /// Run one tile of an image-to-image model. `rgb` is
    /// `size * size * 3` floats in 0..=1; the result is the same times the
    /// spec's scale factor.
    pub fn run_tile(&self, rgb: &[f32]) -> Result<Vec<f32>> {
        let (w, h) = self.input_dims();
        let scale = self.spec.input.scale();
        let (w, h) = (w * scale, h * scale);
        let out = self.run(rgb)?;
        let view = out[0].to_plain_array_view::<f32>()?;
        let shape = view.shape();
        if shape.len() != 4 || shape[1] < 3 {
            bail!("unexpected output shape {shape:?}");
        }
        let (oh, ow) = (shape[2], shape[3]);
        let flat = view.as_slice().context("non-contiguous output")?;
        let range = self.spec.range;
        let mut rgb_out = vec![0.0f32; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                for c in 0..3 {
                    // Some models return a different size than they were
                    // given; clamp rather than fail, so a mismatch shows
                    // as a soft edge and not a crash.
                    let sy = y.min(oh.saturating_sub(1));
                    let sx = x.min(ow.saturating_sub(1));
                    let v = range.decode(flat[((c * oh) + sy) * ow + sx], c);
                    if !v.is_finite() {
                        bail!("model returned a non-finite pixel");
                    }
                    rgb_out[(y * w + x) * 3 + c] = v.clamp(0.0, 1.0);
                }
            }
        }
        Ok(rgb_out)
    }
}

/// How an image was fitted into a model's frame, so an answer in frame
/// coordinates can be read back in image ones.
#[derive(Debug, Clone, Copy)]
struct Framing {
    /// Frame pixels per image pixel, horizontally and vertically. The two
    /// differ when the image was squashed rather than letterboxed.
    scale: (f32, f32),
    /// Where the image starts inside the frame.
    offset: (f32, f32),
}

impl Framing {
    /// The same framing against a map of a different size than the frame.
    ///
    /// A decoder that stops short of its input's resolution -- which is
    /// what the colour network does, because chroma does not need the
    /// resolution -- emits a map whose coordinates are the frame's
    /// scaled down. Folding that in here means the callers all read
    /// their output the same way.
    fn against(self, frame: (usize, usize), out: (usize, usize)) -> Framing {
        let rx = out.0 as f32 / frame.0 as f32;
        let ry = out.1 as f32 / frame.1 as f32;
        Framing {
            scale: (self.scale.0 * rx, self.scale.1 * ry),
            offset: (self.offset.0 * rx, self.offset.1 * ry),
        }
    }
}

/// Resample an image into a model's frame.
///
/// Areas outside a letterboxed image are filled with the mid grey a
/// network reads as "nothing here" rather than with black, which reads as
/// an edge.
fn frame(spec: &ModelSpec, rgb: &[f32], width: usize, height: usize) -> (Vec<f32>, Framing) {
    let (fw, fh) = spec.input.dims();
    let (sx, sy, ox, oy) = match spec.input {
        Input::Frame {
            fit: Fit::Contain, ..
        } => {
            let s = (fw as f32 / width as f32).min(fh as f32 / height as f32);
            (
                s,
                s,
                (fw as f32 - width as f32 * s) / 2.0,
                (fh as f32 - height as f32 * s) / 2.0,
            )
        }
        _ => (
            fw as f32 / width as f32,
            fh as f32 / height as f32,
            0.0,
            0.0,
        ),
    };
    let mut out = vec![0.5f32; fw * fh * 3];
    for fy in 0..fh {
        // Sample at pixel centres, so the resample is not half a pixel
        // off in both directions.
        let iy = (fy as f32 + 0.5 - oy) / sy - 0.5;
        for fx in 0..fw {
            let ix = (fx as f32 + 0.5 - ox) / sx - 0.5;
            if ix < -0.5 || iy < -0.5 || ix > width as f32 - 0.5 || iy > height as f32 - 0.5 {
                continue;
            }
            // Upsampling puts the first centre slightly outside the image.
            // Clamp the coordinate before forming weights, otherwise a negative
            // weight extrapolates colors beyond the source's range.
            let (ix, iy) = (
                ix.clamp(0.0, width as f32 - 1.0),
                iy.clamp(0.0, height as f32 - 1.0),
            );
            let (x0, y0) = (ix.floor() as usize, iy.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
            let (tx, ty) = (ix - x0 as f32, iy - y0 as f32);
            for c in 0..3 {
                let at = |x: usize, y: usize| rgb[(y * width + x) * 3 + c];
                let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
                let bot = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
                out[(fy * fw + fx) * 3 + c] = top * (1.0 - ty) + bot * ty;
            }
        }
    }
    (
        out,
        Framing {
            scale: (sx, sy),
            offset: (ox, oy),
        },
    )
}

type Cache = RwLock<HashMap<String, Option<Arc<Model>>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Fetch a model, loading and caching it on first use.
///
/// Returns `None` when the model is not installed or will not load, which
/// is the signal for a filter to use its classical path instead. The
/// failure is cached too, so a broken file is not re-parsed on every dab.
#[cfg(not(schist_library))]
pub fn get(id: &str) -> Option<Arc<Model>> {
    if let Some(hit) = cache().read().ok()?.get(id) {
        return hit.clone();
    }
    let spec = spec(id)?;
    let loaded = load(spec)
        .map_err(|e| log::warn!("neural model {id}: {e:#}"))
        .ok()
        .map(Arc::new);
    if let Ok(mut c) = cache().write() {
        c.insert(id.to_string(), loaded.clone());
    }
    loaded
}

/// Drop a loaded model from the cache; its memory comes back once any
/// in-flight users let go of their `Arc`s. The next `get` reloads it
/// from disk — callers use this when a model's whole feature has left
/// the screen (the gallery's scorer and search towers are hundreds of
/// resident megabytes between them).
pub fn release(id: &str) {
    if let Ok(mut c) = cache().write() {
        c.remove(id);
    }
}

#[cfg(all(target_arch = "wasm32", not(schist_library)))]
fn load(spec: &'static ModelSpec) -> Result<Model> {
    let store = match web_store().read() {
        Ok(store) => store,
        Err(poisoned) => poisoned.into_inner(),
    };
    let bytes = store
        .get(spec.file)
        .with_context(|| format!("{} is not fetched", spec.file))?;
    Model::from_bytes(spec, bytes)
}

#[cfg(any(not(target_arch = "wasm32"), schist_library))]
fn load(spec: &'static ModelSpec) -> Result<Model> {
    if spec.built_in() {
        let bytes = match spec.id {
            "anti-smudge" => ANTI_SMUDGE_ONNX_XZ,
            "detail" => DETAIL_ONNX,
            "dejpeg" => DEJPEG_ONNX,
            "colorize" => COLORIZE_ONNX,
            "portrait" => PORTRAIT_ONNX,
            "inpaint" => INPAINT_ONNX,
            "detail-matting" => DETAIL_MATTING_ONNX_XZ,
            "matting" => MATTING_ONNX_XZ,
            "subject-guide" => SUBJECT_GUIDE_ONNX_XZ,
            "waifu2x-art" => WAIFU2X_ART_ONNX,
            "waifu2x-photo" => WAIFU2X_PHOTO_ONNX,
            other => bail!("no built-in model named {other}"),
        };
        return Model::from_bytes(spec, bytes);
    }
    let path = model_dir().join(spec.file);
    let bytes = std::fs::read(&path).with_context(|| format!("{}", path.display()))?;
    if spec.source == ModelSource::Generated
        && spec.sha256.is_some_and(|want| sha256_hex(&bytes) != want)
    {
        bail!("local model checksum mismatch: {}", spec.file);
    }
    Model::from_bytes(spec, &bytes)
}

/// Bound the temporary allocation when expanding a compressed model.
const MAX_EXPANDED_MODEL_BYTES: usize = 128 * 1024 * 1024;

/// Expand XZ only while constructing an inference plan. Raw ONNX remains
/// borrowed; the temporary expanded buffer is dropped after parsing. The
/// cached Model owns the plan, so subsequent uses do not decompress again.
fn decode_model_bytes(bytes: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>> {
    use std::io::Read;
    if !bytes.starts_with(b"\xfd7zXZ\0") {
        return Ok(std::borrow::Cow::Borrowed(bytes));
    }
    let mut expanded = Vec::new();
    lzma_rust2::XzReader::new(bytes, false)
        .take(MAX_EXPANDED_MODEL_BYTES as u64 + 1)
        .read_to_end(&mut expanded)
        .context("invalid XZ model")?;
    anyhow::ensure!(
        expanded.len() <= MAX_EXPANDED_MODEL_BYTES,
        "expanded model exceeds 128 MiB"
    );
    Ok(std::borrow::Cow::Owned(expanded))
}

/// Write a downloaded model into the store, rejecting it if the hash does
/// not match what this build expects.
pub fn install(spec: &ModelSpec, bytes: &[u8]) -> Result<PathBuf> {
    if let Some(want) = spec.sha256 {
        let got = sha256_hex(bytes);
        if got != want {
            bail!("checksum mismatch: expected {want}, got {got}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let mut store = match web_store().write() {
            Ok(store) => store,
            Err(poisoned) => poisoned.into_inner(),
        };
        store.insert(spec.file, bytes.to_vec());
        drop(store);
        forget(spec.id);
        Ok(PathBuf::from(spec.file))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dir = model_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(spec.file);
        // Write beside the target and rename, so an interrupted install
        // cannot leave a half-file that later loads as a corrupt model.
        let tmp = path.with_extension("part");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)?;
        forget(spec.id);
        Ok(path)
    }
}

/// Remove an installed model.
pub fn uninstall(spec: &ModelSpec) -> Result<()> {
    // On the web every model is a fetch, the built-ins included, so any
    // of them can be let go of.
    #[cfg(target_arch = "wasm32")]
    {
        let mut store = match web_store().write() {
            Ok(store) => store,
            Err(poisoned) => poisoned.into_inner(),
        };
        store.remove(spec.file);
        drop(store);
        forget(spec.id);
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if spec.built_in() {
            bail!("{} ships with the application", spec.name);
        }
        let path = model_dir().join(spec.file);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        forget(spec.id);
        Ok(())
    }
}

/// Drop a model from the cache so the next use reloads it.
pub fn forget(id: &str) {
    if let Ok(mut c) = cache().write() {
        c.remove(id);
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The size of an installed model on disk, for the manager dialog.
pub fn installed_size(spec: &ModelSpec) -> Option<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        web_store()
            .read()
            .ok()
            .and_then(|m| m.get(spec.file).map(|b| b.len() as u64))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if spec.built_in() {
            return Some(spec.bytes as u64);
        }
        std::fs::metadata(model_dir().join(spec.file))
            .ok()
            .map(|m| m.len())
    }
}

/// Path a model would live at, for messages.
pub fn path_of(spec: &ModelSpec) -> PathBuf {
    model_dir().join(spec.file)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod catalogue_tests {
    use super::*;

    #[test]
    fn foreground_framing_clamps_upsampled_borders() {
        let spec = spec("foreground").unwrap();
        let (rgb, _) = frame(spec, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 2, 1);
        assert_eq!(&rgb[..3], &[0.0; 3]);
        assert_eq!(&rgb[rgb.len() - 3..], &[1.0; 3]);
        assert!(rgb.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    // The web build cannot take these sizes from the embedded bytes (it
    // has none), so the catalogue carries literals; this is what keeps
    // them the truth when a model is retrained.
    #[test]
    fn built_in_sizes_match_the_catalogue() {
        for (id, bytes) in [
            ("anti-smudge", ANTI_SMUDGE_ONNX_XZ),
            ("detail", DETAIL_ONNX),
            ("dejpeg", DEJPEG_ONNX),
            ("colorize", COLORIZE_ONNX),
            ("portrait", PORTRAIT_ONNX),
            ("inpaint", INPAINT_ONNX),
            ("detail-matting", DETAIL_MATTING_ONNX_XZ),
            ("matting", MATTING_ONNX_XZ),
            ("subject-guide", SUBJECT_GUIDE_ONNX_XZ),
            ("waifu2x-art", WAIFU2X_ART_ONNX),
            ("waifu2x-photo", WAIFU2X_PHOTO_ONNX),
        ] {
            assert_eq!(spec(id).unwrap().bytes, bytes.len(), "{id}");
            if let Some(hash) = spec(id).unwrap().sha256 {
                assert_eq!(sha256_hex(bytes), hash, "{id}");
            }
        }
    }

    #[test]
    fn compressed_anti_smudge_matches_the_selected_model() {
        let bytes = decode_model_bytes(ANTI_SMUDGE_ONNX_XZ).unwrap();
        assert_eq!(bytes.len(), 27_185_281);
        assert_eq!(
            sha256_hex(&bytes),
            "553903af0e612c959d087b35cbbcbd00a0d4e890f7c1cc87ef6cef1e54cc218e"
        );
    }

    #[test]
    fn truncated_xz_is_rejected() {
        assert!(decode_model_bytes(&ANTI_SMUDGE_ONNX_XZ[..64]).is_err());
        assert!(
            decode_model_bytes(&ANTI_SMUDGE_ONNX_XZ[..ANTI_SMUDGE_ONNX_XZ.len() - 16]).is_err()
        );
    }

    #[test]
    fn bundled_background_models_expand_to_the_validated_weights() {
        for (bytes, size, hash) in [
            (
                DETAIL_MATTING_ONNX_XZ,
                103_979_583,
                "b6240e8404b30bd94c1e84498a03949b7d2e7e891bed85ed06ac1f8ae1d1dc58",
            ),
            (
                SUBJECT_GUIDE_ONNX_XZ,
                44_091_283,
                "7a7fc4963357feabd82a3b677824349696d740e31ea0e7c6b249ce9ae632270f",
            ),
            (
                MATTING_ONNX_XZ,
                91_521,
                "368329288b05675c70cc7a13fbcb0845eb1ca98620017e640fd2fc70e267073a",
            ),
        ] {
            let expanded = decode_model_bytes(bytes).unwrap();
            assert_eq!(expanded.len(), size);
            assert_eq!(sha256_hex(&expanded), hash);
        }
    }
}

#[cfg(schist_library)]
pub fn get(id: &str) -> Option<Arc<Model>> {
    load(spec(id)?)
        .map(Arc::new)
        .map_err(|e| log::warn!("neural model {id}: {e:#}"))
        .ok()
}
#[cfg(schist_library)]
pub fn installed(id: &str) -> bool {
    spec(id).is_some_and(|s| s.built_in() || model_dir().join(s.file).is_file())
}
