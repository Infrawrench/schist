//! Authoritative CMYK/Lab tile storage, including a separate alpha sample.
use crate::TILE_PIXELS;
use schist_color::{f32_to_u16, f32_to_u8, ColorMode, Depth, NativePixel};

#[derive(Debug, Clone, PartialEq)]
pub enum NativeSamples {
    U8(Box<[u8]>),
    U16(Box<[u16]>),
    F32(Box<[f32]>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativeTile {
    pub mode: ColorMode,
    pub samples: NativeSamples,
}

impl NativeTile {
    pub fn new(mode: ColorMode, depth: Depth) -> Self {
        assert!(matches!(mode, ColorMode::Cmyk | ColorMode::Lab));
        let n = TILE_PIXELS * (mode.channels() + 1);
        Self {
            mode,
            samples: match depth {
                Depth::Eight => NativeSamples::U8(vec![0; n].into_boxed_slice()),
                Depth::Sixteen => NativeSamples::U16(vec![0; n].into_boxed_slice()),
                Depth::ThirtyTwo => NativeSamples::F32(vec![0.0; n].into_boxed_slice()),
            },
        }
    }
    pub fn depth(&self) -> Depth {
        match self.samples {
            NativeSamples::U8(_) => Depth::Eight,
            NativeSamples::U16(_) => Depth::Sixteen,
            NativeSamples::F32(_) => Depth::ThirtyTwo,
        }
    }
    pub fn byte_len(&self) -> usize {
        TILE_PIXELS * (self.mode.channels() + 1) * self.depth().bytes_per_channel()
    }
    pub fn get(&self, ix: usize) -> NativePixel {
        let n = self.mode.channels();
        let offset = ix * (n + 1);
        let sample = |i| match &self.samples {
            NativeSamples::U8(b) => b[offset + i] as f32 / 255.0,
            NativeSamples::U16(b) => b[offset + i] as f32 / 65535.0,
            NativeSamples::F32(b) => b[offset + i],
        };
        let mut px = NativePixel::transparent(self.mode);
        for i in 0..n {
            px.color[i] = sample(i);
        }
        px.alpha = sample(n);
        px
    }
    pub fn set(&mut self, ix: usize, px: NativePixel) {
        assert_eq!(px.mode, self.mode);
        let n = self.mode.channels();
        let offset = ix * (n + 1);
        for i in 0..=n {
            let v = if i == n { px.alpha } else { px.color[i] };
            match &mut self.samples {
                NativeSamples::U8(b) => b[offset + i] = f32_to_u8(v),
                NativeSamples::U16(b) => b[offset + i] = f32_to_u16(v),
                NativeSamples::F32(b) => b[offset + i] = v,
            }
        }
    }
}
