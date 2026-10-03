//! Animated WebP, lossless.
//!
//! `image-webp` encodes single lossless (VP8L) images but has no animation
//! writer, so this builds the extended container around its output, per
//! Google's published RIFF container specification
//! (<https://developers.google.com/speed/webp/docs/riff_container>):
//! a `VP8X` header with the animation flag, an `ANIM` chunk with the loop
//! count, and one `ANMF` chunk per frame wrapping that frame's `VP8L`
//! bitstream. Lossy frames would need a VP8 encoder, which no pure-Rust
//! crate provides, so animated WebP export is lossless only.

use std::io::Write;

use schist_core::animation::LoopCount;

use crate::RenderedFrame;

const ANIMATION: u8 = 0x02;
const ALPHA: u8 = 0x10;
/// ANMF flags: bit 1 set means "do not blend" -- the frame replaces the
/// canvas under it -- and bit 0 clear means "do not dispose".
const NO_BLEND: u8 = 0x02;
/// A frame duration is a 24-bit millisecond count.
const MAX_DURATION: u32 = (1 << 24) - 1;

fn u24(v: u32) -> [u8; 3] {
    let b = v.to_le_bytes();
    [b[0], b[1], b[2]]
}

fn chunk(out: &mut Vec<u8>, fourcc: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(fourcc);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        out.push(0);
    }
}

/// One frame's VP8L bitstream, taken out of the simple-format file the
/// encoder writes (`RIFF` size `WEBP` `VP8L` size payload).
fn vp8l(rgba: &[u8], width: u32, height: u32) -> anyhow::Result<Vec<u8>> {
    let mut file = Vec::new();
    image_webp::WebPEncoder::new(&mut file).encode(
        rgba,
        width,
        height,
        image_webp::ColorType::Rgba8,
    )?;
    anyhow::ensure!(
        file.len() >= 20 && &file[12..16] == b"VP8L",
        "unexpected WebP encoder output"
    );
    let len = u32::from_le_bytes(file[16..20].try_into()?) as usize;
    file.get(20..20 + len)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| anyhow::anyhow!("truncated WebP encoder output"))
}

/// Encode a lossless animated WebP.
pub fn encode_webp(
    frames: &[RenderedFrame],
    width: u32,
    height: u32,
    loop_count: LoopCount,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        width <= 1 << 14 && height <= 1 << 14,
        "WebP is limited to 16384 pixels a side"
    );
    let mut body = Vec::new();
    let alpha = frames
        .iter()
        .any(|f| f.rgba.chunks_exact(4).any(|p| p[3] != 255));
    let mut vp8x = vec![ANIMATION | if alpha { ALPHA } else { 0 }, 0, 0, 0];
    vp8x.extend_from_slice(&u24(width - 1));
    vp8x.extend_from_slice(&u24(height - 1));
    chunk(&mut body, b"VP8X", &vp8x);
    // Background colour (ignored by browsers, transparent here) then the
    // loop count: zero is forever, otherwise total plays.
    let mut anim = vec![0, 0, 0, 0];
    let plays = loop_count.plays().unwrap_or(0).min(u16::MAX as u32) as u16;
    anim.write_all(&plays.to_le_bytes())?;
    chunk(&mut body, b"ANIM", &anim);
    for frame in frames {
        anyhow::ensure!(
            frame.rgba.len() == (width * height * 4) as usize,
            "frame size does not match the animation"
        );
        let mut anmf = Vec::new();
        anmf.extend_from_slice(&u24(0)); // x / 2
        anmf.extend_from_slice(&u24(0)); // y / 2
        anmf.extend_from_slice(&u24(width - 1));
        anmf.extend_from_slice(&u24(height - 1));
        anmf.extend_from_slice(&u24(frame.delay_ms.min(MAX_DURATION)));
        anmf.push(NO_BLEND);
        chunk(&mut anmf, b"VP8L", &vp8l(&frame.rgba, width, height)?);
        chunk(&mut body, b"ANMF", &anmf);
    }
    let mut out = Vec::with_capacity(body.len() + 12);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(body.len() as u32 + 4).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    Ok(out)
}
