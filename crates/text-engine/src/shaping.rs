//! Unicode paragraph itemization, OpenType shaping and vertical placement.
//! Ordinary Latin specs without overrides keep their historical geometry.
use super::*;
use unicode_bidi::{BidiInfo, Level};
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;
use unicode_vo::{char_orientation, Orientation};

pub(super) fn bidi_control(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

pub(super) fn required(spec: &TextSpec) -> bool {
    !spec.inline_objects.is_empty()
        || (spec.tabs.is_some() && spec.text.contains('\t'))
        || !super::language::effective(&spec.language).is_empty()
        || spec.runs.iter().any(|r| {
            r.language
                .as_deref()
                .is_some_and(|v| !super::language::effective(v).is_empty())
        })
        || !spec.features.is_empty()
        || spec.runs.iter().any(|run| {
            !run.features.is_empty()
                || run
                    .capitalization
                    .is_some_and(|v| v != Capitalization::Normal)
        })
        || spec.direction != ParagraphDirection::Auto
        || spec.writing_mode.is_vertical()
        || spec.text.chars().any(|c| {
            !matches!(
                c.script(),
                Script::Latin | Script::Common | Script::Inherited
            ) || (c != '\n'
                && (unicode_bidi::bidi_class(c) == unicode_bidi::BidiClass::B || c == '\u{2028}'))
                || bidi_control(c)
        })
}

#[derive(Default)]
struct Shaped {
    glyphs: Vec<PlacedGlyph>,
    chars: Vec<CharPos>,
    width: f32,
    discretionary_hyphen: bool,
    // Accumulate advances before rounding glyph coordinates. A distant styled
    // prefix must not change the spacing inside a following shaping item.
    precise_width: f64,
    tab_stops: Vec<(usize, usize)>,
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
    hyphen: Option<usize>,
) -> Vec<(usize, usize, Orientation)> {
    // Hidden soft hyphens must not split a shaping item. In particular, an
    // unscaled invisible glyph between synthetic small caps must not interrupt
    // their font run and kerning. A selected visible hyphen retains its own face.
    let chars: Vec<_> = spec.text[start..end]
        .char_indices()
        .filter(|(offset, c)| *c != '\u{ad}' || hyphen == Some(start + offset))
        .collect();
    let mut script = chars
        .iter()
        .map(|(_, c)| c.script())
        .find(|s| !matches!(s, Script::Common | Script::Inherited))
        .unwrap_or(Script::Common);
    let mut out = Vec::new();
    let mut begin = start;
    let mut style_start = chars.first().map_or(start, |(offset, _)| start + offset);
    let mut face = faces.at(style_start);
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
            && (spec.shaping_context(begin) != spec.shaping_context(at)
                || face != faces.at(at)
                || !spec.style_at(style_start).shapes_like(&spec.style_at(at))
                || next_script != script
                || (spec.writing_mode.is_vertical()
                    && (next_vertical != vertical
                        || next_vertical == Orientation::TransformedOrRotated)))
        {
            out.push((begin, at, vertical));
            begin = at;
            style_start = at;
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
    range: std::ops::Range<usize>,
    rtl: bool,
    vertical: Orientation,
    hyphen: Option<usize>,
    out: &mut Shaped,
) {
    let (start, end) = (range.start, range.end);
    let visible_start = spec.text[start..end]
        .char_indices()
        .find(|(offset, c)| *c != '\u{ad}' || hyphen == Some(start + offset))
        .map_or(start, |(offset, _)| start + offset);
    let ix = faces.at(visible_start);
    let (loaded, size) = &faces.faces[ix];
    let Some(face) = rustybuzz::Face::from_slice(&loaded.data, loaded.index) else {
        return;
    };
    let scale = size / face.units_per_em() as f32;
    let text = &spec.text[start..end];
    let ttb = spec.writing_mode.is_vertical() && vertical != Orientation::Rotated;
    let style = spec.style_at(visible_start);
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    for (offset, c) in text.char_indices() {
        if hyphen == Some(start + offset) {
            // Keep the source cluster (and its two-byte caret range), while
            // choosing the visible glyph in the same font and shaping run.
            buffer.add('-', offset as u32);
        } else if faces
            .uppercase
            .get(start + offset)
            .copied()
            .unwrap_or(false)
        {
            let context = spec.shaping_context(start + offset);
            for upper in super::language::uppercase(
                &spec.text[context.clone()],
                start + offset - context.start,
                &style.language,
            ) {
                buffer.add(upper, offset as u32);
            }
        } else {
            buffer.add(c, offset as u32);
        }
    }
    let context = spec.shaping_context(start);
    buffer.set_pre_context(&spec.text[context.start..start]);
    buffer.set_post_context(&spec.text[end..context.end]);
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
        return shape_item(
            spec,
            faces,
            start..end,
            rtl,
            Orientation::Rotated,
            hyphen,
            out,
        );
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
        let visible = |byte: usize| {
            let c = spec.text[byte..].chars().next().unwrap();
            !bidi_control(c) && (c != '\u{ad}' || hyphen == Some(byte))
        };
        let visible_count = char_bytes.iter().filter(|byte| visible(**byte)).count();
        let x = out.precise_width;
        for j in i..next {
            if visible_count == 0 {
                continue;
            }
            let pos = positions[j];
            out.glyphs.push(PlacedGlyph {
                glyph: infos[j].glyph_id as u16,
                byte: start + cluster,
                // During shaping, x is the inline position and baseline is
                // the cross-axis offset. layout converts them to canvas axes.
                x: (out.precise_width
                    + f64::from(if ttb {
                        -pos.y_offset as f32 * scale
                    } else {
                        pos.x_offset as f32 * scale
                    })) as f32,
                baseline: if ttb {
                    pos.x_offset as f32 * scale
                } else {
                    -pos.y_offset as f32 * scale
                },
                face: ix,
                sideways: spec.writing_mode.is_vertical() && !ttb,
            });
            out.precise_width += f64::from(if ttb {
                -pos.y_advance as f32 * scale
            } else {
                pos.x_advance as f32 * scale
            });
        }
        out.precise_width +=
            f64::from(spec.style_at(start + cluster).tracking) * visible_count as f64;
        out.precise_width += f64::from(spec.word_spacing)
            * text[cluster..cluster_end]
                .char_indices()
                .filter(|(at, ch)| *ch == ' ' && !spec.in_object(start + cluster + at))
                .count() as f64;
        let count = visible_count.max(1) as f64;
        let mut k = 0;
        for byte in char_bytes {
            let next = k + usize::from(visible(byte));
            let (a, b) = if rtl && !ttb {
                (count - k as f64, count - next as f64)
            } else {
                (k as f64, next as f64)
            };
            out.chars.push(CharPos {
                byte,
                x: (x + (out.precise_width - x) * a / count) as f32,
                end_x: (x + (out.precise_width - x) * b / count) as f32,
            });
            k = next;
        }
        i = next;
    }
    out.width = out.precise_width as f32;
}

fn shape_plain(
    spec: &TextSpec,
    faces: &Faces,
    bidi: &BidiInfo<'_>,
    paragraph_start: usize,
    start: usize,
    end: usize,
    hyphen: Option<usize>,
) -> Shaped {
    let mut out = Shaped {
        discretionary_hyphen: hyphen.is_some_and(|at| (start..end).contains(&at)),
        ..Default::default()
    };
    if start == end {
        return out;
    }
    // A chosen discretionary glyph belongs to the preceding word's resolved
    // direction. Leaving it as BN lets UAX #9 L1 reset it to the paragraph
    // direction, moving a Latin word's hyphen to its left in an RTL paragraph.
    // This is a typography policy, not an alteration of the source bidi text:
    // only the display glyph's class/level changes, at its original byte range.
    let display_bidi = hyphen.filter(|at| *at > start && *at < end).map(|at| {
        let at = at - paragraph_start;
        let level = bidi.levels[at - 1];
        let mut display = BidiInfo {
            text: bidi.text,
            original_classes: bidi.original_classes.clone(),
            levels: bidi.levels.clone(),
            paragraphs: bidi.paragraphs.clone(),
        };
        display.levels[at..at + '\u{ad}'.len_utf8()].fill(level);
        display.original_classes[at..at + '\u{ad}'.len_utf8()].fill(if level.is_rtl() {
            unicode_bidi::BidiClass::R
        } else {
            unicode_bidi::BidiClass::L
        });
        display
    });
    let bidi = display_bidi.as_ref().unwrap_or(bidi);
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
            hyphen,
        );
        if rtl {
            items.reverse();
        }
        for (start, end, vertical) in items {
            shape_item(spec, faces, start..end, rtl, vertical, hyphen, &mut out);
        }
    }
    out
}

/// A single leader unit is shaped independently with the tab's resolved style.
/// Its glyphs stay in inline/cross coordinates until their eventual line paint.
pub(super) fn leader_pattern(spec: &TextSpec, faces: &Faces) -> (Vec<PlacedGlyph>, f32) {
    let bidi = BidiInfo::new(
        &spec.text,
        Some(if spec.direction == ParagraphDirection::RightToLeft {
            Level::rtl()
        } else {
            Level::ltr()
        }),
    );
    let shaped = shape_plain(spec, faces, &bidi, 0, 0, spec.text.len(), None);
    (shaped.glyphs, shaped.width)
}

/// Tabs separate shaping fields but retain paragraph bidi context and source
/// byte positions. Each tab anchors the following shaped field, so font/style
/// runs, ligatures and vertical metrics also determine aligned tab positions.
fn shape(
    spec: &TextSpec,
    faces: &Faces,
    bidi: &BidiInfo<'_>,
    paragraph_start: usize,
    line: impl Into<soft_hyphen::Line>,
    start: f32,
    limit: Option<f32>,
) -> Shaped {
    let line = line.into();
    let range = line.range;
    let hyphen = (spec.text[range.clone()].ends_with('\u{ad}')
        && (line.hyphen || (range.end == spec.text.len() && spec.show_final_soft_hyphen)))
        .then_some(range.end.saturating_sub('\u{ad}'.len_utf8()));
    let Some(tabs) = spec
        .tabs
        .as_ref()
        .filter(|_| spec.text[range.clone()].contains('\t'))
    else {
        return shape_plain(
            spec,
            faces,
            bidi,
            paragraph_start,
            range.start,
            range.end,
            hyphen,
        );
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
    let mut selected_stops = Vec::new();
    let mut from = range.start;
    let mut pen = 0.0;
    // Bidi changes field order across a horizontal ruler. Vertical inline
    // progression remains downward, independently of paragraph direction.
    let reverse_fields = !spec.writing_mode.is_vertical() && bidi.paragraphs[0].level.is_rtl();
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
        let field = shape_plain(field_spec, faces, bidi, paragraph_start, from, at, hyphen);
        if from > range.start {
            let Some((mut next, stop)) = tabs.next_stop(pen, start, |alignment| match alignment {
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
                        .map_or(field.width, |c| {
                            if reverse_fields {
                                field.width - c.x
                            } else {
                                c.x
                            }
                        })
                }
            }) else {
                return Shaped {
                    width: f32::INFINITY,
                    ..Default::default()
                };
            };
            // A terminal tab is a break opportunity even when its stop is
            // beyond the line edge. Keep its source/caret and bound its gap;
            // the following field can then start on the next line. A leading
            // tab alone cannot manufacture a blank line to consume overset.
            if from == range.end
                && spec.text[range.start..from - 1]
                    .chars()
                    .any(|c| !c.is_whitespace())
            {
                if let Some(width) = limit.or(tabs.line_width) {
                    if pen <= width && next > width {
                        next = width;
                    }
                }
            }
            tab_positions.push(CharPos {
                byte: from - 1,
                x: pen,
                end_x: next,
            });
            if let Some(stop) = stop {
                selected_stops.push((from - 1, stop));
            }
            pen = next;
        }
        let next_pen = pen + field.width;
        fields.push((pen, next_pen, field));
        pen = next_pen;
        if at < range.end {
            from = at + 1;
        }
    }
    let mut out = Shaped {
        width: pen,
        discretionary_hyphen: hyphen.is_some(),
        tab_stops: selected_stops,
        ..Default::default()
    };
    for (offset, end, mut field) in fields {
        let offset = if reverse_fields {
            // Use the stored edge, avoiding a second subtraction of the
            // field width. Cancellation near zero can otherwise floor an
            // entire glyph mask into the preceding pixel.
            pen - end
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
    if reverse_fields {
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

pub(super) fn layout(
    spec: &TextSpec,
    base: &LoadedFace,
    measures: &[InlineMeasure],
    policy: &super::hyphenation::BreakPolicy<'_>,
) -> Layout {
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
        if paragraph.contains('\u{ad}') {
            let first = lines.len();
            let mut boundaries = spec
                .object_breaks(start..end)
                .into_iter()
                .chain(
                    soft_hyphen::hebrew_breaks(paragraph)
                        .into_iter()
                        .map(|at| start + at),
                )
                .filter(|at| *at == end || spec.allows_wrap_at(*at))
                .collect::<Vec<_>>();
            boundaries.sort_unstable();
            boundaries.dedup();
            let shaped = |range, hyphen, i| {
                shape(
                    spec,
                    &faces,
                    &bidi,
                    start,
                    soft_hyphen::Line { range, hyphen },
                    inline_start(first + i),
                    wrap_width_at(spec, &widths, first + i),
                )
            };
            let selected = soft_hyphen::lines(
                &spec.text,
                start..end,
                &boundaries,
                |i| {
                    (spec.path.is_none() || spec.writing_mode.is_vertical())
                        .then(|| wrap_width_at(spec, &widths, first + i))
                        .flatten()
                },
                |range, hyphen, i| shaped(range, hyphen, i).width,
                policy,
            );
            for (i, line) in selected.into_iter().enumerate() {
                lines.push((
                    line.range.start,
                    line.range.end,
                    shaped(line.range, line.hyphen, i),
                ));
            }
            continue;
        }
        let mut line_start = start;
        if (spec.path.is_none() || spec.writing_mode.is_vertical())
            && wrap_width_at(spec, &widths, lines.len()).is_some()
        {
            let mut previous = start;
            for at in spec.object_breaks(start..end) {
                if at < end && !spec.allows_wrap_at(at) {
                    continue;
                }
                let candidate = shape(
                    spec,
                    &faces,
                    &bidi,
                    start,
                    line_start..at,
                    inline_start(lines.len()),
                    wrap_width_at(spec, &widths, lines.len()),
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
                            wrap_width_at(spec, &widths, lines.len()),
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
                wrap_width_at(spec, &widths, lines.len()),
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
        tab_stops: Vec::new(),
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
            discretionary_hyphen: line.discretionary_hyphen,
            generated_hyphen: false,
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
        out.tab_stops.extend(line.tab_stops);
        out.lines.push(LineSpan {
            start,
            end,
            discretionary_hyphen: line.discretionary_hyphen,
            generated_hyphen: false,
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
