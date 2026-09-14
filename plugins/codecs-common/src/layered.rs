//! Shared bounds and export checks for the native layered raster codecs.
use anyhow::{ensure, Result};
use schist_core::{Document, IntRect, Layer, LayerKind};
use schist_i18n::{t, tf};

pub const MAX_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_LAYERS: usize = 4096;

pub fn invalid() -> anyhow::Error {
    anyhow::anyhow!("{}", t("codec.layered.invalid"))
}

pub fn unsupported(feature: &str) -> anyhow::Error {
    anyhow::anyhow!("{}", tf!("codec.layered.unsupported", feature = feature))
}

pub fn size(w: u32, h: u32, bpp: usize) -> Result<usize> {
    ensure!(w > 0 && h > 0, "{}", t("codec.msg.zero_sized"));
    // Leave headroom for signed document coordinates and tile rounding.
    ensure!(
        w <= 1_000_000 && h <= 1_000_000,
        "{}",
        t("codec.layered.too_large")
    );
    let len = (w as usize)
        .checked_mul(h as usize)
        .and_then(|n| n.checked_mul(bpp))
        .ok_or_else(invalid)?;
    ensure!(len <= MAX_BYTES, "{}", t("codec.layered.too_large"));
    Ok(len)
}

pub fn budget(remaining: &mut usize, bytes: usize) -> Result<()> {
    *remaining = remaining
        .checked_sub(bytes)
        .ok_or_else(|| anyhow::anyhow!("{}", t("codec.layered.too_large")))?;
    Ok(())
}

pub fn tile_bytes(bounds: IntRect, depth: schist_color::Depth, mask: bool) -> usize {
    let bytes_per_pixel = if mask {
        1
    } else {
        match depth {
            schist_color::Depth::Eight => 4,
            schist_color::Depth::Sixteen => 8,
            schist_color::Depth::ThirtyTwo => 16,
        }
    };
    schist_core::TileCoord::covering(&bounds)
        .count()
        .checked_mul(schist_core::TILE_PIXELS)
        .and_then(|n| n.checked_mul(bytes_per_pixel))
        .unwrap_or(usize::MAX)
}

pub fn rect(x: i32, y: i32, w: u32, h: u32) -> Result<IntRect> {
    size(w, h, 4)?;
    ensure!(
        x.abs_diff(0) <= 1_000_000 && y.abs_diff(0) <= 1_000_000,
        "{}",
        t("codec.layered.too_large")
    );
    Ok(IntRect::from_xywh(x, y, w, h))
}

pub fn finish(mut doc: Document) -> Document {
    if doc.active_layer.is_none() {
        doc.active_layer = doc.tree.layers.last().map(|l| l.id);
    }
    doc.damage_all();
    doc.mark_saved();
    doc
}

pub fn check_layer(layer: &Layer, groups: bool, masks: bool) -> Result<()> {
    ensure!(
        matches!(layer.kind, LayerKind::Raster(_)) || (groups && layer.is_group()),
        "{}",
        tf!("codec.layered.export_layer", name = layer.name)
    );
    ensure!(
        !layer.clipping
            && layer.style.is_empty()
            && layer.blending_ranges.is_empty()
            && (masks || layer.mask.is_none())
            && layer.shape.is_none()
            && layer.smart.is_none()
            && layer.raw.is_none()
            && layer.extras.is_empty(),
        "{}",
        tf!("codec.layered.export_layer", name = layer.name)
    );
    Ok(())
}

pub struct Reader<'a> {
    pub bytes: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub fn at(bytes: &'a [u8], pos: usize) -> Result<Self> {
        ensure!(
            pos > 0 && pos < bytes.len(),
            "{}",
            t("codec.layered.invalid")
        );
        Ok(Self { bytes, pos })
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(invalid)?;
        let out = self.bytes.get(self.pos..end).ok_or_else(invalid)?;
        self.pos = end;
        Ok(out)
    }
    pub fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn be32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?))
    }
    pub fn le32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into()?))
    }
    pub fn pointer(&mut self, wide: bool) -> Result<usize> {
        let n = if wide {
            u64::from_be_bytes(self.take(8)?.try_into()?)
        } else {
            self.be32()? as u64
        };
        usize::try_from(n).map_err(|_| invalid())
    }
    pub fn xcf_string(&mut self) -> Result<String> {
        let n = self.be32()? as usize;
        if n == 0 {
            return Ok(String::new());
        }
        let data = self.take(n)?;
        ensure!(data.last() == Some(&0), "{}", t("codec.layered.invalid"));
        Ok(String::from_utf8_lossy(&data[..n - 1]).into_owned())
    }
}
