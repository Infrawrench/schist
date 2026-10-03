//! Animated PNG: full 8-bit RGBA per frame, lossless.

use schist_core::animation::LoopCount;

use crate::RenderedFrame;

/// An APNG delay as a fraction of a second. Milliseconds fit the 16-bit
/// numerator up to a minute; past that, centiseconds do.
fn delay(ms: u32) -> (u16, u16) {
    if ms <= u16::MAX as u32 {
        (ms as u16, 1000)
    } else {
        ((ms / 10).min(u16::MAX as u32) as u16, 100)
    }
}

/// Encode an APNG. Every frame covers the canvas and replaces what was
/// there (`APNG_BLEND_OP_SOURCE`), so transparent areas never accumulate
/// earlier frames.
pub fn encode_apng(
    frames: &[RenderedFrame],
    width: u32,
    height: u32,
    loop_count: LoopCount,
) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(frames.len() as u32, loop_count.plays().unwrap_or(0))?;
        let mut writer = encoder.write_header()?;
        for frame in frames {
            anyhow::ensure!(
                frame.rgba.len() == (width * height * 4) as usize,
                "frame size does not match the animation"
            );
            let (num, den) = delay(frame.delay_ms);
            writer.set_frame_delay(num, den)?;
            writer.set_blend_op(png::BlendOp::Source)?;
            writer.set_dispose_op(png::DisposeOp::None)?;
            writer.write_image_data(&frame.rgba)?;
        }
        writer.finish()?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn long_delays_switch_to_centiseconds() {
        assert_eq!(super::delay(100), (100, 1000));
        assert_eq!(super::delay(65_535), (65_535, 1000));
        assert_eq!(super::delay(120_000), (12_000, 100));
    }
}
