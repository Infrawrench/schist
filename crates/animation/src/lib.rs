//! Frame animation output: rendering a document's frames, and writing
//! them as animated GIF, APNG or animated WebP.
//!
//! The frame model and its undoable operations live in the kernel
//! ([`schist_core::animation`]); this crate adds what needs a compositor
//! or an encoder. Every encoder here is pure Rust, so exports work the
//! same natively and in the browser.

mod apng;
mod gif_export;
mod render;
mod webp;

pub use apng::encode_apng;
pub use gif_export::{encode_gif, Disposal, GifOptions, PaletteMode};
pub use render::{
    downscale, flatten_frames, render_frame, render_frames, tint, RenderedFrame, ONION_AFTER,
    ONION_BEFORE,
};
pub use webp::encode_webp;

use schist_core::animation::LoopCount;

/// The formats an animation can be exported as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Gif,
    Apng,
    WebP,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Gif, Format::Apng, Format::WebP];

    pub fn extension(self) -> &'static str {
        match self {
            Format::Gif => "gif",
            Format::Apng => "png",
            Format::WebP => "webp",
        }
    }

    /// The format's own name, which is not translated.
    pub fn name(self) -> &'static str {
        match self {
            Format::Gif => "GIF",
            Format::Apng => "APNG",
            Format::WebP => "WebP",
        }
    }
}

/// Everything an export needs besides the frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportOptions {
    pub format: Format,
    pub gif: GifOptions,
    /// Keep transparent pixels transparent. Off, every frame is flattened
    /// onto `matte` first, which is what a GIF without a transparent index
    /// needs and what some players expect of APNG and WebP too.
    pub transparency: bool,
    pub matte: [u8; 3],
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            format: Format::Gif,
            gif: GifOptions::default(),
            transparency: true,
            matte: [255, 255, 255],
        }
    }
}

/// Encode rendered frames in `options.format`.
pub fn encode(
    frames: &[RenderedFrame],
    width: u32,
    height: u32,
    loop_count: LoopCount,
    options: &ExportOptions,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(!frames.is_empty(), "an animation needs at least one frame");
    anyhow::ensure!(width > 0 && height > 0, "an animation needs a size");
    let flattened;
    let frames = if options.transparency {
        frames
    } else {
        flattened = frames
            .iter()
            .map(|f| RenderedFrame {
                rgba: render::over_matte(&f.rgba, options.matte),
                delay_ms: f.delay_ms,
            })
            .collect::<Vec<_>>();
        &flattened[..]
    };
    match options.format {
        Format::Gif => encode_gif(frames, width, height, loop_count, &options.gif),
        Format::Apng => encode_apng(frames, width, height, loop_count),
        Format::WebP => encode_webp(frames, width, height, loop_count),
    }
}
