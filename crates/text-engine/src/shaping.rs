//! Unicode paragraph itemization, OpenType shaping and vertical placement.
//! Ordinary Latin specs without overrides keep their historical geometry.
use super::*;
use unicode_bidi::{BidiInfo, Level};
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;
use unicode_vo::{char_orientation, Orientation};

pub(super) fn required(spec: &TextSpec) -> bool {
    (spec.tabs.is_some() && spec.text.contains('\t'))
        || !super::language::effective(&spec.language).is_empty()
        || spec.runs.iter().any(|r| r.language.as_deref().is_some_and(|v| !super::language::effective(v).is_empty()))
        || !spec.features.is_empty()
        || spec.runs.iter().any(|run| !run.features.is_empty() || run.capitalization.is_some_and(|v| v != Capitalization::Normal))
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

fn features(style: &CharStyle) -> Vec<rustybuzz::Feature> {
    // Preserve the editor's discretionary Latin ligature default. Required
    // Arabic ligatures remain enabled by rustybuzz's script shaper.
    let mut features = vec![
        rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(b"liga"), 0, ..),
        rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(b"clig"), 0, ..),
    ];
    for f in &capitalization::features(style) {
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
                || !spec.style_at(begin).shapes_like(&spec.style_at(at))
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
    let style = spec.style_at(start);
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    for (offset, c) in text.char_indices() {
        if faces
            .uppercase
            .get(start + offset)
            .copied()
            .unwrap_or(false)
        {
            for upper in super::language::uppercase(&spec.text, start + offset, &style.language) {
                buffer.add(upper, offset as u32);
            }
        } else {
            buffer.add(c, offset as u32);
        }
    }
    buffer.set_pre_context(&spec.text[..start]);
    buffer.set_post_context(&spec.text[end..]);
    buffer.guess_segment_properties();
    if let Ok(language) = style.language.parse() {
        buffer.set_language(language);
    }
    buffer.set_direction(if ttb {
        rustybuzz::Direction::TopToBottom
    } else if rtl {
        rustybuzz::Direction::RightToLeft
    } else {
        rustybuzz::Direction::LeftToRight
    });
    let shaped = rustybuzz::shape(&face, &features(&style), buffer);
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
                byte: start + cluster,
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
        out.width += spec.style_at(start + cluster).tracking * char_bytes.len() as f32;
        out.width += spec.word_spacing
            * text[cluster..cluster_end]
                .chars()
                .filter(|ch| *ch == ' ')
                .count() as f32;
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

fn shape_plain(
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

/// Tabs separate shaping fields but retain paragraph bidi context and source
/// byte positions. Each tab anchors the following shaped field, so font/style
/// runs, ligatures and vertical metrics also determine aligned tab positions.
fn shape(
    spec: &TextSpec,
    faces: &Faces,
    bidi: &BidiInfo<'_>,
    paragraph_start: usize,
    range: std::ops::Range<usize>,
    start: f32,
) -> Shaped {
    let Some(tabs) = spec
        .tabs
        .as_ref()
        .filter(|_| spec.text[range.clone()].contains('\t'))
    else {
        return shape_plain(spec, faces, bidi, paragraph_start, range.start, range.end);
    };
    // Justification expands only the final field. Earlier fields must not grow
    // past their tab stops and jump to a different stop during painting.
    let unspaced = (spec.word_spacing != 0.0).then(|| {
        let mut copy = spec.clone();
        copy.word_spacing = 0.0;
        copy
    });
    let mut fields = Vec::new();
    let mut tab_positions = Vec::new();
    let mut from = range.start;
    let mut pen = 0.0;
    for at in spec.text[range.clone()]
        .char_indices()
        .filter_map(|(i, c)| (c == '\t').then_some(range.start + i))
        .chain(std::iter::once(range.end))
    {
        let field_spec = if at < range.end {
            unspaced.as_ref().unwrap_or(spec)
        } else {
            spec
        };
        let field = shape_plain(field_spec, faces, bidi, paragraph_start, from, at);
        if from > range.start {
            let Some(next) = tabs.next_aligned(pen, start, |alignment| match alignment {
                TabAlignment::Leading => 0.0,
                TabAlignment::Trailing => field.width,
                TabAlignment::Center => field.width / 2.0,
                TabAlignment::Character(character) => {
                    // Anchor the first matching source character at its
                    // grapheme caret, including a character inside a ligature.
                    // If absent, Schist uses the field end (native behavior is
                    // unverified). Never synthesize a source byte position.
                    spec.text[from..at]
                        .find(character)
                        .and_then(|byte| {
                            field
                                .chars
                                .iter()
                                .filter(|c| c.byte <= from + byte)
                                .max_by_key(|c| c.byte)
                        })
                        .map_or(field.width, |c| c.x)
                }
            }) else {
                return Shaped {
                    width: f32::INFINITY,
                    ..Default::default()
                };
            };
            tab_positions.push(CharPos {
                byte: from - 1,
                x: pen,
                end_x: next,
            });
            pen = next;
        }
        let next_pen = pen + field.width;
        fields.push((pen, field));
        pen = next_pen;
        if at < range.end {
            from = at + 1;
        }
    }
    let rtl = bidi.paragraphs[0].level.is_rtl();
    let mut out = Shaped {
        width: pen,
        ..Default::default()
    };
    for (offset, mut field) in fields {
        let offset = if rtl {
            pen - offset - field.width
        } else {
            offset
        };
        for glyph in &mut field.glyphs {
            glyph.x += offset;
        }
        for character in &mut field.chars {
            character.x += offset;
            character.end_x += offset;
        }
        out.glyphs.extend(field.glyphs);
        out.chars.extend(field.chars);
    }
    if rtl {
        for character in &mut tab_positions {
            character.x = pen - character.x;
            character.end_x = pen - character.end_x;
        }
    }
    out.chars.extend(tab_positions);
    // Paint grouping follows visual adjacency, including styled tab spaces.
    out.chars.sort_by(|a, b| {
        a.x.min(a.end_x)
            .total_cmp(&b.x.min(b.end_x))
            .then(a.byte.cmp(&b.byte))
    });
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

pub(super) fn layout(spec: &TextSpec, base: &LoadedFace, measures: &[InlineMeasure]) -> Layout {
    let widths = measures.iter().map(|m| m.width).collect::<Vec<_>>();
    let inline_start = |index| {
        measures
            .get(index)
            .or_else(|| measures.last())
            .map_or(0.0, |m| m.start)
    };
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
        if (spec.path.is_none() || spec.writing_mode.is_vertical())
            && wrap_width_at(spec, &widths, lines.len()).is_some()
        {
            let mut previous = start;
            for (boundary, _) in unicode_linebreak::linebreaks(paragraph) {
                let at = start + boundary;
                let candidate = shape(
                    spec,
                    &faces,
                    &bidi,
                    start,
                    line_start..at,
                    inline_start(lines.len()),
                );
                let limit = wrap_width_at(spec, &widths, lines.len()).unwrap();
                if previous > line_start && candidate.width > limit {
                    lines.push((
                        line_start,
                        previous,
                        shape(
                            spec,
                            &faces,
                            &bidi,
                            start,
                            line_start..previous,
                            inline_start(lines.len()),
                        ),
                    ));
                    line_start = previous;
                }
                previous = at;
            }
        }
        lines.push((
            line_start,
            end,
            shape(
                spec,
                &faces,
                &bidi,
                start,
                line_start..end,
                inline_start(lines.len()),
            ),
        ));
    }
    if lines.iter().any(|(_, _, line)| !line.width.is_finite()) {
        return Layout::default();
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
    let absolute = spec.has_absolute_leading();
    let mut geometry = Vec::with_capacity(lines.len());
    let mut metrics = Vec::with_capacity(lines.len());
    for (start, end, line) in &lines {
        let (ascent, step) = spec.text[*start..*end]
            .char_indices()
            .map(|(k, _)| faces.line_metrics_at(spec, *start + k))
            .reduce(|(a, h), (b, j)| (a.max(b), h.max(j)))
            .unwrap_or_else(|| faces.line_metrics(0));
        let advance = run_line_advance(spec, &faces, *start, *end, step);
        let height = if absolute { step } else { advance };
        let top = next_line_top(
            geometry.last(),
            ascent,
            height,
            advance,
            spec.writing_mode,
            absolute,
        );
        geometry.push(LineSpan {
            start: *start,
            end: *end,
            x: 0.0,
            width: line.width,
            top,
            baseline: top + ascent,
            height,
            advance,
        });
        metrics.push((ascent, step));
    }
    let total_height = block_extent(&geometry);
    for (i, ((start, end, mut line), span)) in lines.into_iter().zip(geometry).enumerate() {
        let (ascent, step) = metrics[i];
        let (top, height) = (span.top, span.height);
        if i == 0 {
            out.first_baseline = ascent;
            out.line_advance = span.advance;
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
                g.x = center + cross + spec.style_at(g.byte).baseline_shift;
                g.baseline = inline;
            } else {
                g.x += x;
                g.baseline += top + ascent - spec.style_at(g.byte).baseline_shift;
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
            baseline: top + ascent,
            height,
            advance: span.advance,
        });
    }
    out
}
