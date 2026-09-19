//! Unicode paragraph itemization, OpenType shaping and vertical placement.
//! Ordinary Latin specs without overrides keep their historical geometry.
use super::*;
use unicode_bidi::{BidiInfo, Level};
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;
use unicode_vo::{char_orientation, Orientation};

pub(super) fn required(spec: &TextSpec) -> bool {
    !spec.features.is_empty()
        || spec.direction != ParagraphDirection::Auto
        || spec.writing_mode.is_vertical()
        || spec.text.chars().any(|c| {
            !matches!(c.script(), Script::Latin | Script::Common | Script::Inherited)
                || (c != '\n' && (unicode_bidi::bidi_class(c) == unicode_bidi::BidiClass::B || c == '\u{2028}'))
                || matches!(c, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

#[derive(Default)]
struct Shaped {
    glyphs: Vec<PlacedGlyph>,
    chars: Vec<CharPos>,
    width: f32,
}

fn features(spec: &TextSpec) -> Vec<rustybuzz::Feature> {
    // Preserve the editor's discretionary Latin ligature default. Required
    // Arabic ligatures remain enabled by rustybuzz's script shaper.
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
    features
}

/// Itemize a bidi run further by font, script and vertical orientation. Common
/// and inherited characters follow their surrounding script, so Arabic marks
/// stay in the same shaping buffer as their base letters.
fn items(
    spec: &TextSpec,
    faces: &Faces,
    start: usize,
    end: usize,
) -> Vec<(usize, usize, Orientation)> {
    let chars: Vec<_> = spec.text[start..end].char_indices().collect();
    let mut script = chars
        .iter()
        .map(|(_, c)| c.script())
        .find(|s| !matches!(s, Script::Common | Script::Inherited))
        .unwrap_or(Script::Common);
    let mut out = Vec::new();
    let mut begin = start;
    let mut face = faces.at(start);
    let mut vertical = chars
        .first()
        .map_or(Orientation::Upright, |(_, c)| char_orientation(*c));
    for (offset, c) in chars {
        let at = start + offset;
        let next_script = match c.script() {
            Script::Common | Script::Inherited => script,
            s => s,
        };
        let next_vertical = if c.script() == Script::Inherited {
            vertical
        } else {
            char_orientation(c)
        };
        if at > begin
            && (face != faces.at(at)
                || next_script != script
                || (spec.writing_mode.is_vertical()
                    && (next_vertical != vertical
                        || next_vertical == Orientation::TransformedOrRotated)))
        {
            out.push((begin, at, vertical));
            begin = at;
        }
        face = faces.at(at);
        script = next_script;
        vertical = next_vertical;
    }
    if begin < end {
        out.push((begin, end, vertical));
    }
    out
}

fn shape_item(
    spec: &TextSpec,
    faces: &Faces,
    start: usize,
    end: usize,
    rtl: bool,
    vertical: Orientation,
    out: &mut Shaped,
) {
    let ix = faces.at(start);
    let (loaded, size) = &faces.faces[ix];
    let Some(face) = rustybuzz::Face::from_slice(&loaded.data, loaded.index) else {
        return;
    };
    let scale = size / face.units_per_em() as f32;
    let text = &spec.text[start..end];
    let ttb = spec.writing_mode.is_vertical() && vertical != Orientation::Rotated;
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_pre_context(&spec.text[..start]);
    buffer.set_post_context(&spec.text[end..]);
    buffer.guess_segment_properties();
    buffer.set_direction(if ttb {
        rustybuzz::Direction::TopToBottom
    } else if rtl {
        rustybuzz::Direction::RightToLeft
    } else {
        rustybuzz::Direction::LeftToRight
    });
    let shaped = rustybuzz::shape(&face, &features(spec), buffer);
    // UAX #50 Tr uses the vertical alternate when the font has one, and a
    // clockwise horizontal glyph otherwise. Do not leave brackets upright
    // in fonts with no vert/vrt2 substitution.
    if ttb
        && vertical == Orientation::TransformedOrRotated
        && shaped.glyph_infos().iter().all(|g| {
            text[g.cluster as usize..]
                .chars()
                .next()
                .and_then(|c| face.glyph_index(c))
                .is_some_and(|id| id.0 as u32 == g.glyph_id)
        })
    {
        return shape_item(spec, faces, start, end, rtl, Orientation::Rotated, out);
    }
    let infos = shaped.glyph_infos();
    let positions = shaped.glyph_positions();
    // In RTL output clusters decrease. Their source end is the next higher
    // cluster, never the next output glyph (which would slice backwards).
    let mut boundaries: Vec<_> = infos
        .iter()
        .map(|g| g.cluster as usize)
        .chain(std::iter::once(text.len()))
        .collect();
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut i = 0;
    while i < infos.len() {
        let cluster = infos[i].cluster as usize;
        let mut next = i + 1;
        while next < infos.len() && infos[next].cluster == infos[i].cluster {
            next += 1;
        }
        let cluster_end = boundaries[boundaries.partition_point(|b| *b <= cluster)];
        let char_bytes: Vec<_> = text[cluster..cluster_end]
            .grapheme_indices(true)
            .map(|(k, _)| start + cluster + k)
            .collect();
        let x = out.width;
        for j in i..next {
            let pos = positions[j];
            out.glyphs.push(PlacedGlyph {
                glyph: infos[j].glyph_id as u16,
                // During shaping, x is the inline position and baseline is
                // the cross-axis offset. layout converts them to canvas axes.
                x: out.width
                    + if ttb {
                        -pos.y_offset as f32 * scale
                    } else {
                        pos.x_offset as f32 * scale
                    },
                baseline: if ttb {
                    pos.x_offset as f32 * scale
                } else {
                    -pos.y_offset as f32 * scale
                },
                face: ix,
                sideways: spec.writing_mode.is_vertical() && !ttb,
            });
            out.width += if ttb {
                -pos.y_advance as f32 * scale
            } else {
                pos.x_advance as f32 * scale
            };
        }
        out.width += spec.tracking * char_bytes.len() as f32;
        let count = char_bytes.len().max(1) as f32;
        for (k, byte) in char_bytes.into_iter().enumerate() {
            let (a, b) = if rtl && !ttb {
                (count - k as f32, count - k as f32 - 1.0)
            } else {
                (k as f32, k as f32 + 1.0)
            };
            out.chars.push(CharPos {
                byte,
                x: x + (out.width - x) * a / count,
                end_x: x + (out.width - x) * b / count,
            });
        }
        i = next;
    }
}

fn shape(
    spec: &TextSpec,
    faces: &Faces,
    bidi: &BidiInfo<'_>,
    paragraph_start: usize,
    start: usize,
    end: usize,
) -> Shaped {
    let mut out = Shaped::default();
    if start == end {
        return out;
    }
    let (levels, runs) = bidi.visual_runs(
        &bidi.paragraphs[0],
        start - paragraph_start..end - paragraph_start,
    );
    for run in runs {
        let rtl = levels[run.start].is_rtl();
        let mut items = items(
            spec,
            faces,
            paragraph_start + run.start,
            paragraph_start + run.end,
        );
        if rtl {
            items.reverse();
        }
        for (start, end, vertical) in items {
            shape_item(spec, faces, start, end, rtl, vertical, &mut out);
        }
    }
    out
}

/// Preserve source offsets, including CRLF as a single paragraph separator.
fn paragraphs(text: &str) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if unicode_bidi::bidi_class(c) == unicode_bidi::BidiClass::B || c == '\u{2028}' {
            result.push((start, at));
            start = at + c.len_utf8();
            if c == '\r' && chars.peek().is_some_and(|(_, c)| *c == '\n') {
                chars.next();
                start += 1;
            }
        }
    }
    result.push((start, text.len()));
    result
}

pub(super) fn paragraph_is_rtl(spec: &TextSpec, byte: usize) -> bool {
    let level = match spec.direction {
        ParagraphDirection::Auto => None,
        ParagraphDirection::LeftToRight => Some(Level::ltr()),
        ParagraphDirection::RightToLeft => Some(Level::rtl()),
    };
    let bidi = BidiInfo::new(&spec.text, level);
    bidi.paragraphs
        .iter()
        .rev()
        .find(|p| p.range.start <= byte)
        .is_some_and(|p| p.level.is_rtl())
}

pub(super) fn layout(spec: &TextSpec, base: &LoadedFace) -> Layout {
    let faces = Faces::resolve(spec, base);
    let mut lines = Vec::new();
    let level = match spec.direction {
        ParagraphDirection::Auto => None,
        ParagraphDirection::LeftToRight => Some(Level::ltr()),
        ParagraphDirection::RightToLeft => Some(Level::rtl()),
    };
    for (start, end) in paragraphs(&spec.text) {
        let paragraph = &spec.text[start..end];
        let bidi = BidiInfo::new(paragraph, level);
        let mut line_start = start;
        if let Some(limit) = spec
            .wrap_width
            .filter(|_| spec.path.is_none() || spec.writing_mode.is_vertical())
        {
            let mut previous = start;
            for (boundary, _) in unicode_linebreak::linebreaks(paragraph) {
                let at = start + boundary;
                let candidate = shape(spec, &faces, &bidi, start, line_start, at);
                if previous > line_start && candidate.width > limit {
                    lines.push((
                        line_start,
                        previous,
                        shape(spec, &faces, &bidi, start, line_start, previous),
                    ));
                    line_start = previous;
                }
                previous = at;
            }
        }
        lines.push((
            line_start,
            end,
            shape(spec, &faces, &bidi, start, line_start, end),
        ));
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
    let total_height: f32 = lines
        .iter()
        .map(|(start, end, _)| {
            spec.text[*start..*end]
                .char_indices()
                .map(|(k, _)| faces.line_metrics(faces.at(start + k)).1)
                .reduce(f32::max)
                .unwrap_or_else(|| faces.line_metrics(0).1)
                * spec.line_height.max(0.1)
        })
        .sum();
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
            if spec.writing_mode.is_vertical() {
                let inline = g.x + x;
                let cross = if g.sideways {
                    // Centre the horizontal em box across the column.
                    -g.baseline + (step / 2.0 - ascent)
                } else {
                    g.baseline
                };
                let center = if spec.writing_mode == WritingMode::VerticalRl {
                    total_height - top - height / 2.0
                } else {
                    top + height / 2.0
                };
                g.x = center + cross;
                g.baseline = inline;
            } else {
                g.x += x;
                g.baseline += top + ascent;
            }
        }
        for c in &mut line.chars {
            c.x += x;
            c.end_x += x;
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
