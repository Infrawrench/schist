//! Opt-in OpenType shaping. Specs without overrides retain their original
//! layout, including imported documents whose geometry was fitted to it.
use super::*;

struct Shaped {
    glyphs: Vec<PlacedGlyph>,
    chars: Vec<CharPos>,
    width: f32,
}

fn shape(spec: &TextSpec, faces: &Faces, start: usize, end: usize) -> Shaped {
    // Match the legacy defaults unless the user explicitly enables ligatures.
    let mut features = vec![
        rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(b"liga"), 0, ..),
        rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(b"clig"), 0, ..),
    ];
    for f in &spec.features {
        if f.tag.len() == 4 && f.tag.bytes().all(|b| b.is_ascii_graphic()) {
            let tag = ttf_parser::Tag::from_bytes(f.tag.as_bytes().try_into().unwrap());
            features.retain(|f| f.tag != tag);
            features.push(rustybuzz::Feature::new(tag, f.value, ..));
        }
    }
    let mut out = Shaped {
        glyphs: Vec::new(),
        chars: Vec::new(),
        width: 0.0,
    };
    let mut at = start;
    while at < end {
        let ix = faces.at(at);
        let run_end = spec.text[at..end]
            .char_indices()
            .find(|(k, _)| faces.at(at + k) != ix)
            .map_or(end, |(k, _)| at + k);
        let (loaded, size) = &faces.faces[ix];
        let Some(face) = rustybuzz::Face::from_slice(&loaded.data, loaded.index) else {
            break;
        };
        let scale = size / face.units_per_em() as f32;
        let text = &spec.text[at..run_end];
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        // The editor currently lays out horizontal LTR paragraphs. Bidi
        // requires paragraph itemization as well as a glyph shaper.
        buffer.set_direction(rustybuzz::Direction::LeftToRight);
        let shaped = rustybuzz::shape(&face, &features, buffer);
        let infos = shaped.glyph_infos();
        let positions = shaped.glyph_positions();
        let mut i = 0;
        while i < infos.len() {
            let cluster = infos[i].cluster as usize;
            let mut next = i + 1;
            while next < infos.len() && infos[next].cluster == infos[i].cluster {
                next += 1;
            }
            let cluster_end = infos.get(next).map_or(text.len(), |g| g.cluster as usize);
            let char_bytes: Vec<_> = text[cluster..cluster_end]
                .char_indices()
                .map(|(k, _)| at + cluster + k)
                .collect();
            let x = out.width;
            for j in i..next {
                let pos = positions[j];
                out.glyphs.push(PlacedGlyph {
                    glyph: infos[j].glyph_id as u16,
                    x: out.width + pos.x_offset as f32 * scale,
                    baseline: -pos.y_offset as f32 * scale,
                    face: ix,
                });
                out.width += pos.x_advance as f32 * scale;
            }
            out.width += spec.tracking * char_bytes.len() as f32;
            // Distribute insertion points within a ligature. Byte offsets
            // stay UTF-8 boundaries even when several characters share ink.
            let count = char_bytes.len().max(1) as f32;
            for (k, byte) in char_bytes.into_iter().enumerate() {
                out.chars.push(CharPos {
                    byte,
                    x: x + (out.width - x) * k as f32 / count,
                });
            }
            i = next;
        }
        at = run_end;
    }
    out
}

pub(super) fn layout(spec: &TextSpec, base: &LoadedFace) -> Layout {
    let faces = Faces::resolve(spec, base);
    let mut lines = Vec::new();
    let mut start = 0;
    for paragraph in spec.text.split('\n') {
        let mut line_start = start;
        let mut at = start;
        let mut width = 0.0;
        for word in paragraph.split_inclusive(' ') {
            let word_width = shape(spec, &faces, at, at + word.len()).width;
            if spec.path.is_none()
                && spec
                    .wrap_width
                    .is_some_and(|w| at > line_start && width + word_width > w)
            {
                lines.push((line_start, at, shape(spec, &faces, line_start, at)));
                line_start = at;
                width = 0.0;
            }
            width += word_width;
            at += word.len();
        }
        lines.push((line_start, at, shape(spec, &faces, line_start, at)));
        start = at + 1;
    }
    let max_width = lines.iter().map(|(_, _, l)| l.width).fold(0.0f32, f32::max);
    let mut out = Layout {
        glyphs: Vec::new(),
        chars: Vec::new(),
        lines: Vec::new(),
        first_baseline: 0.0,
        line_advance: 0.0,
        layout_width: max_width,
    };
    let mut top = 0.0;
    for (i, (start, end, mut line)) in lines.into_iter().enumerate() {
        let (ascent, step) = spec.text[start..end]
            .char_indices()
            .map(|(k, _)| faces.line_metrics(faces.at(start + k)))
            .reduce(|(a, h), (b, j)| (a.max(b), h.max(j)))
            .unwrap_or_else(|| faces.line_metrics(0));
        let height = step * spec.line_height.max(0.1);
        if i == 0 {
            out.first_baseline = ascent;
            out.line_advance = height;
        }
        let x = match spec.align {
            Align::Left => 0.0,
            Align::Center => (max_width - line.width) / 2.0,
            Align::Right => max_width - line.width,
        };
        for g in &mut line.glyphs {
            g.x += x;
            g.baseline += top + ascent;
        }
        for c in &mut line.chars {
            c.x += x;
        }
        out.glyphs.extend(line.glyphs);
        out.chars.extend(line.chars);
        out.lines.push(LineSpan {
            start,
            end,
            x,
            width: line.width,
            top,
            height,
        });
        top += height;
    }
    out
}
