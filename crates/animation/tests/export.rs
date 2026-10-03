//! Encode real document animations, then decode them with independent
//! decoders: frame count, delays, loop count and transparency.

use schist_animation::{
    encode, render_frames, Disposal, ExportOptions, Format, GifOptions, PaletteMode,
};
use schist_color::{Depth, Rgba};
use schist_core::animation::{self, LoopCount};
use schist_core::{blit_rgba8, Document, IntRect, Layer};

const W: u32 = 24;
const H: u32 = 16;

fn square(name: &str, rect: IntRect, color: Rgba) -> Layer {
    let mut layer = Layer::new_raster(name);
    let px = color.to_u8();
    let rgba: Vec<u8> = (0..rect.width() * rect.height()).flat_map(|_| px).collect();
    blit_rgba8(
        &mut layer.as_raster_mut().unwrap().tiles,
        Depth::Eight,
        rect,
        &rgba,
    );
    layer
}

/// Three frames on a transparent canvas: a red square, a green one, and
/// the red one again moved 8 px right by a frame offset.
fn doc(loop_count: LoopCount) -> Document {
    let mut doc = Document::new("anim", W, H, Depth::Eight);
    let red = doc.push_layer(square(
        "red",
        IntRect::from_xywh(2, 2, 6, 6),
        Rgba::new(1.0, 0.0, 0.0, 1.0),
    ));
    doc.push_layer(square(
        "green",
        IntRect::from_xywh(10, 4, 6, 6),
        Rgba::new(0.0, 1.0, 0.0, 1.0),
    ));
    animation::make_frames_from_layers(&mut doc, "make").unwrap();
    animation::select(&mut doc, 0, "select").unwrap_err();
    animation::step(&mut doc, 1, "next").unwrap();
    animation::step(&mut doc, -1, "previous").unwrap();
    animation::duplicate(&mut doc, "new").unwrap();
    animation::reorder(&mut doc, 1, 2, "move").unwrap();
    animation::set_offset(&mut doc, red, (8, 0), "offset").unwrap();
    let _ = animation::set_delay(&mut doc, Some(0), 100, "delay");
    animation::set_delay(&mut doc, Some(1), 250, "delay").unwrap();
    animation::set_delay(&mut doc, Some(2), 40, "delay").unwrap();
    let _ = animation::set_loop(&mut doc, loop_count, "loop");
    doc
}

fn pixel(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    rgba[i..i + 4].try_into().unwrap()
}

#[test]
fn rendered_frames_follow_visibility_and_offsets() {
    let frames = render_frames(&doc(LoopCount::Forever)).unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(
        frames.iter().map(|f| f.delay_ms).collect::<Vec<_>>(),
        [100, 250, 40]
    );
    assert_eq!(pixel(&frames[0].rgba, 4, 4), [255, 0, 0, 255]);
    assert_eq!(
        pixel(&frames[0].rgba, 12, 6)[3],
        0,
        "green hidden in frame 1"
    );
    assert_eq!(pixel(&frames[1].rgba, 12, 6), [0, 255, 0, 255]);
    assert_eq!(pixel(&frames[1].rgba, 4, 4)[3], 0);
    // Frame 3 is frame 1 with the red square offset by 8 px.
    assert_eq!(pixel(&frames[2].rgba, 4, 4)[3], 0);
    assert_eq!(pixel(&frames[2].rgba, 12, 4), [255, 0, 0, 255]);
}

fn gif_decode(
    bytes: &[u8],
) -> (
    Vec<Vec<u8>>,
    Vec<u16>,
    gif::Repeat,
    Vec<gif::DisposalMethod>,
) {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(bytes).unwrap();
    let (mut frames, mut delays, mut disposal) = (Vec::new(), Vec::new(), Vec::new());
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        assert_eq!((frame.width as u32, frame.height as u32), (W, H));
        frames.push(frame.buffer.to_vec());
        delays.push(frame.delay);
        disposal.push(frame.dispose);
    }
    (frames, delays, decoder.repeat(), disposal)
}

#[test]
fn gif_round_trip_with_both_palette_modes() {
    for (loop_count, repeat) in [
        (LoopCount::Forever, gif::Repeat::Infinite),
        (LoopCount::Times(3), gif::Repeat::Finite(2)),
        (LoopCount::Times(1), gif::Repeat::Finite(0)),
    ] {
        let doc = doc(loop_count);
        let frames = render_frames(&doc).unwrap();
        for palette in [PaletteMode::PerFrame, PaletteMode::Global] {
            let options = ExportOptions {
                format: Format::Gif,
                gif: GifOptions {
                    palette,
                    dither: true,
                    disposal: Disposal::Auto,
                },
                ..Default::default()
            };
            let bytes = encode(&frames, W, H, loop_count, &options).unwrap();
            let (decoded, delays, got_repeat, disposal) = gif_decode(&bytes);
            assert_eq!(decoded.len(), 3);
            assert_eq!(delays, [10, 25, 4]);
            assert_eq!(got_repeat, repeat);
            assert!(disposal
                .iter()
                .all(|d| *d == gif::DisposalMethod::Background));
            // Exact colours (few of them) and real transparency.
            assert_eq!(pixel(&decoded[0], 4, 4), [255, 0, 0, 255]);
            assert_eq!(pixel(&decoded[0], 20, 14)[3], 0);
            assert_eq!(pixel(&decoded[1], 12, 6), [0, 255, 0, 255]);
            assert_eq!(pixel(&decoded[2], 12, 4), [255, 0, 0, 255]);
        }
        // Without transparency the matte fills the background and the
        // frames need no disposal.
        let options = ExportOptions {
            transparency: false,
            ..Default::default()
        };
        let bytes = encode(&frames, W, H, loop_count, &options).unwrap();
        let (decoded, _, _, disposal) = gif_decode(&bytes);
        assert_eq!(pixel(&decoded[0], 20, 14), [255, 255, 255, 255]);
        assert!(disposal.iter().all(|d| *d == gif::DisposalMethod::Keep));
    }
}

#[test]
fn apng_round_trip() {
    for (loop_count, plays) in [(LoopCount::Forever, 0), (LoopCount::Times(2), 2)] {
        let doc = doc(loop_count);
        let frames = render_frames(&doc).unwrap();
        let options = ExportOptions {
            format: Format::Apng,
            ..Default::default()
        };
        let bytes = encode(&frames, W, H, loop_count, &options).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        let mut reader = decoder.read_info().unwrap();
        let control = reader.info().animation_control.unwrap();
        assert_eq!((control.num_frames, control.num_plays), (3, plays));
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        for (i, want) in frames.iter().enumerate() {
            reader.next_frame(&mut buf).unwrap();
            let fc = reader.info().frame_control.unwrap();
            assert_eq!(
                (fc.delay_num as u32 * 1000) / fc.delay_den as u32,
                want.delay_ms
            );
            assert_eq!(
                buf[..want.rgba.len()],
                want.rgba[..],
                "frame {i} is lossless"
            );
        }
    }
}

#[test]
fn webp_round_trip() {
    for (loop_count, want_loop) in [
        (LoopCount::Forever, image_webp::LoopCount::Forever),
        (
            LoopCount::Times(4),
            image_webp::LoopCount::Times(4.try_into().unwrap()),
        ),
    ] {
        let doc = doc(loop_count);
        let frames = render_frames(&doc).unwrap();
        let options = ExportOptions {
            format: Format::WebP,
            ..Default::default()
        };
        let bytes = encode(&frames, W, H, loop_count, &options).unwrap();
        let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(&bytes)).unwrap();
        assert!(decoder.is_animated());
        assert!(decoder.has_alpha());
        assert_eq!(decoder.dimensions(), (W, H));
        assert_eq!(decoder.num_frames(), 3);
        assert_eq!(decoder.loop_count(), want_loop);
        let mut buf = vec![0; (W * H * 4) as usize];
        for want in &frames {
            let delay = decoder.read_frame(&mut buf).unwrap();
            assert_eq!(delay, want.delay_ms);
            assert_eq!(buf, want.rgba, "lossless");
        }
    }
}

/// Cross-check with ffprobe and ImageMagick when they are installed. The
/// files are written to `SCHIST_ANIMATION_ARTIFACT_DIR` when it is set, so
/// they can be opened in a browser too.
#[test]
fn external_tools_agree_when_installed() {
    let dir = std::env::var_os("SCHIST_ANIMATION_ARTIFACT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("schist-animation-export"));
    std::fs::create_dir_all(&dir).unwrap();
    let doc = doc(LoopCount::Times(3));
    // The source document too, for opening the timeline in the app.
    std::fs::write(
        dir.join("anim.psd"),
        schist_codec_psd::write_psd(&doc).unwrap(),
    )
    .unwrap();
    let frames = render_frames(&doc).unwrap();
    for format in Format::ALL {
        let options = ExportOptions {
            format,
            ..Default::default()
        };
        let bytes = encode(&frames, W, H, LoopCount::Times(3), &options).unwrap();
        let path = dir.join(format!("anim.{}", format.extension()));
        std::fs::write(&path, bytes).unwrap();
        if let Ok(out) = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
            .args(["-show_entries", "stream=nb_read_frames,width,height"])
            .args(["-of", "csv=p=0"])
            .arg(&path)
            .output()
        {
            // ffmpeg has no animated WebP demuxer for ANMF frames.
            if out.status.success() && format != Format::WebP {
                let text = String::from_utf8_lossy(&out.stdout);
                assert_eq!(text.trim(), format!("{W},{H},3"), "{format:?}");
            }
        }
        if let Ok(out) = std::process::Command::new("identify")
            .args(["-format", "%T %W %H\n"])
            .arg(&path)
            .output()
        {
            if out.status.success() && matches!(format, Format::Gif | Format::WebP) {
                let text = String::from_utf8_lossy(&out.stdout);
                let delays: Vec<_> = text.lines().map(|l| l.split(' ').next().unwrap()).collect();
                assert_eq!(delays, ["10", "25", "4"], "{text}");
            }
        }
    }
}
