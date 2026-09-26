//! Schist kernel: document model, tiles, layers, selection, history.
//!
//! This crate deliberately contains **no user-facing features** — those live
//! in plugins (see `schist-plugin-api`). If every plugin were removed the
//! app would boot to an empty workspace that can do nothing.

pub mod annotate;
pub mod blend;
pub mod creative;
pub mod curves;
pub mod document;
pub mod filter_stack;
pub mod geom;
pub mod history;
pub mod ink;
pub mod layer;
pub mod vector_blend;
pub use ink::{InkChannel, InkChannelInfo, InkPreview, InkTiles};
pub mod live_mask;
pub mod mask_refine;
pub mod model3d;
pub mod path;
pub mod raw;
pub mod resample;
pub mod selection;
pub mod smart;
pub mod smart_source;
pub mod style;
pub mod tile;

pub use annotate::{
    Artboard, CountGroup, LayerComp, LayerCompState, Note, Slice, DEFAULT_NOTE_COLOR,
};
pub use blend::BlendMode;
pub use document::{
    blit_rgba8, blit_rgba_f32, Document, DocumentId, EditBuilder, Guide, PreservedResource,
    StrokeEdit,
};
pub use geom::IntRect;
pub use history::{Edit, EditOp, History, LayerProps};
pub use layer::{
    AdjustmentData, AdjustmentKind, GroupLayer, Layer, LayerId, LayerKind, LayerMask, LayerPath,
    LayerTree, RasterLayer, RawBlock, StyledRaster,
};
pub use path::{Anchor, SubPath, VectorPath, VectorShape};
pub use raw::{RawDevelopment, RawSettings};
pub use resample::{Affine, Filter};
pub use selection::{SelectOp, Selection};
pub use smart::SmartObject;
pub use style::{
    BevelStyle, BevelStyle_, BlurStyle, ColorOverlayStyle, Effect, GlowStyle, GradientOverlayStyle,
    GradientShape, LayerStyle, SatinStyle, ShadowStyle, StrokePosition, StrokeStyle, Technique,
};
pub use tile::{MaskTileMap, TileBuf, TileCoord, TileMap, TILE_PIXELS, TILE_SIZE};

pub use schist_color as color;

pub mod native;
pub use native::{NativeSamples, NativeTile};

/// Stateless identifiers for independently embedded libraries. Keep the integer
/// exactly representable by JavaScript (53 bits), with zero reserved.
#[cfg(schist_library)]
pub fn fresh_id() -> u64 {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).expect("operating system entropy unavailable");
    (u64::from_le_bytes(bytes) & ((1 << 53) - 1)).max(1)
}

/// GPU programs for color classification and connected selection growth.
pub mod selection_gpu;
