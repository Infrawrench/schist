//! Native editable objects. Layout donors are existing, Affinity-written probes.
use super::*;

impl Exporter {
    pub(super) fn editable_node(
        &mut self,
        layer: &Layer,
        clips: &[&Layer],
        origin: (f64, f64),
    ) -> Option<usize> {
        if layer.mask.is_some() || !clips.is_empty() {
            return None;
        }
        if layer
            .extras
            .iter()
            .find(|b| b.key == *b"AfNs")
            .is_some_and(|b| b.data == editable_snapshot(layer))
        {
            let raw = &layer.extras.iter().find(|b| b.key == *b"AfNt")?.data;
            let index = self.decode_native(raw)?;
            let xf = match self.g.nodes[index].field(b"Xfrm") {
                Some(Value::VecD(v)) if v.len() == 6 => {
                    [v[0], v[1], v[2] - origin.0, v[3], v[4], v[5] - origin.1]
                }
                _ => [1.0, 0.0, -origin.0, 0.0, 1.0, -origin.1],
            };
            self.finish_editable(index, layer, clips, origin, xf);
            return Some(index);
        }
        if layer.shape.is_some() {
            return self.curve_node(layer, origin);
        }
        self.text_node(layer, origin)
    }

    fn decode_native(&mut self, raw: &[u8]) -> Option<usize> {
        let next = &mut self.next_id;
        crate::preserve::decode(raw, &mut self.g, &mut || {
            let id = *next;
            *next += 1;
            id
        })
        .map(|d| d.root)
    }

    fn replace_field(&mut self, index: usize, field: Field) {
        let n = &mut self.g.nodes[index];
        if let Some(at) = n.fields.iter().position(|(t, _)| *t == field.0) {
            n.fields[at].1 = field.3;
            n.wire[at] = field.1;
            n.aux[at] = field.2;
        } else {
            n.fields.push((field.0, field.3));
            n.wire.push(field.1);
            n.aux.push(field.2);
        }
    }

    fn finish_editable(
        &mut self,
        index: usize,
        layer: &Layer,
        clips: &[&Layer],
        origin: (f64, f64),
        xf: [f64; 6],
    ) {
        // Absence is Affinity's normal-blend default. Do not retain an old
        // explicit blend when the user switched the layer back to Normal.
        if let Some(at) = self.g.nodes[index]
            .fields
            .iter()
            .position(|(key, _)| *key == tag(b"Blnd"))
        {
            let node = &mut self.g.nodes[index];
            node.fields.remove(at);
            node.wire.remove(at);
            node.aux.remove(at);
        }
        let fields = self.common_fields(layer, None, None);
        for field in fields {
            self.replace_field(index, field);
        }
        self.replace_field(index, f(b"Xfrm", 0x28, Value::VecD(xf.to_vec())));
        if [tag(b"TxtF"), tag(b"TxtA")].contains(&self.g.nodes[index].type_tag()) {
            let flow = self.push_node_closed(
                &[(b"TxFl", 1)],
                vec![
                    f(b"Nods", 0xb1, Value::Array(vec![Self::class(index)])),
                    f_aux(b"HOvr", 0x29, 1, Value::Bool(true)),
                ],
            );
            self.replace_field(index, f(b"Flow", 0x31, Self::class(flow)));
        }
        // New curves use document coordinates. Retained objects can have an
        // affine local basis; masks/clips require inverse mapping to that basis.
        // Those objects are deliberately rasterized when attachments are present.
        let children = self.build_stack_refs(clips, origin);
        self.replace_field(index, f(b"Chld", 0xb1, Value::Array(children)));
        let mask = self.mask_field(layer, origin);
        if non_empty_mask_slot(&mask) {
            self.replace_field(index, mask);
        }
    }
}

impl Exporter {
    fn solid_fill(&mut self, color: schist_color::Rgba) -> usize {
        let rgba = self.rgba_node(color);
        self.push_node(
            &[(b"FilS", 1), (b"Fill", 0)],
            vec![f(b"Colr", 0x31, Self::class(rgba))],
        )
    }

    fn curve_node(&mut self, layer: &Layer, origin: (f64, f64)) -> Option<usize> {
        let shape = layer.shape.as_ref()?;
        // The probe establishes one contour's winding; compound fill-rule
        // variants need their own reference before they can be authored.
        if shape.path.subpaths.len() != 1 {
            return None;
        }
        let sub = &shape.path.subpaths[0];
        if sub.anchors.len() < 2 || sub.anchors.len() > 100_000 {
            return None;
        }
        let record = |point: (f32, f32), marker: [u8; 2]| -> Option<Value> {
            if !point.0.is_finite() || !point.1.is_finite() {
                return None;
            }
            let mut raw = (point.0 as f64).to_le_bytes().to_vec();
            raw.extend_from_slice(&(point.1 as f64).to_le_bytes());
            raw.extend_from_slice(&marker);
            Some(Value::Curve(raw))
        };
        let mut records = vec![record(sub.anchors[0].point, [1, 0])?];
        for i in 0..sub.anchors.len() {
            let a = sub.anchors[i];
            let next = (i + 1) % sub.anchors.len();
            if next == 0 && !sub.closed {
                break;
            }
            let b = sub.anchors[next];
            records.push(record(
                (a.point.0 + a.handle_out.0, a.point.1 + a.handle_out.1),
                [0, 1],
            )?);
            records.push(record(
                (b.point.0 + b.handle_in.0, b.point.1 + b.handle_in.1),
                [0, 2],
            )?);
            if next != 0 {
                records.push(record(
                    b.point,
                    [if next + 1 == sub.anchors.len() { 1 } else { 2 }, 0],
                )?);
            }
        }
        let data = self.push_node_framed(
            0x30,
            &[],
            ChainEnd::None,
            0,
            vec![
                (0, 0x01, 0, Value::U8(0)),
                (0, 0x03, 0, Value::U32(1)),
                (0, 0x29, 0, Value::Bool(sub.closed)),
                (0, 0xac, 18, Value::Array(records)),
            ],
        );
        let curves = self.push_tagged(
            (b"PCvD", 1),
            vec![f_aux(b"Data", 0x30, 18, Self::class(data))],
        );
        let fill = self.solid_fill(shape.fill);
        let mut fields = self.common_fields(
            layer,
            Some([1.0, 0.0, -origin.0, 0.0, 1.0, -origin.1]),
            None,
        );
        fields.push(f(b"BFil", 0x31, Self::class(fill)));
        if let Some((color, width)) = shape.stroke {
            if !width.is_finite() || width < 0.0 {
                return None;
            }
            let pen = self.solid_fill(color);
            // LSty's probe record: f64 line parameter, cap, join, style,
            // reserved. Reuse the observed round-cap/join (2,2) convention.
            let mut data = 2.0f64.to_le_bytes().to_vec();
            data.extend_from_slice(&[2, 2, 1, 0]);
            let style = self.push_node_closed(
                &[(b"LSty", 1)],
                vec![
                    f(b"Data", 0x2c, Value::Curve(data)),
                    f(b"Wght", 0x0a, Value::F64(width as f64)),
                    f(b"Brus", 0x31, Value::Class(None)),
                ],
            );
            let descriptor = self.push_node_closed(
                &[(b"LDsc", 1)],
                vec![
                    f(b"LDeL", 0x31, Self::class(style)),
                    f(b"LDBe", 0x29, Value::Bool(false)),
                    f(b"LDSc", 0x29, Value::Bool(false)),
                    f(b"LDeP", 0x32, Value::Class(None)),
                    f(b"LDSa", 0x07, Value::I32(0)),
                ],
            );
            fields.extend([
                f(b"PFil", 0x31, Self::class(pen)),
                f(b"LSty", 0x31, Self::class(descriptor)),
            ]);
        }
        let bounds = shape.path.bounds();
        fields.extend([
            f_aux(b"Crvs", 0x32, 18, Self::class(curves)),
            f(
                b"CvsB",
                0x26,
                Value::VecD(vec![
                    bounds.left as f64,
                    bounds.top as f64,
                    bounds.right as f64,
                    bounds.bottom as f64,
                ]),
            ),
            f(b"CvWi", 0x2a, Value::Enum { id: 0, version: 0 }),
            f(b"ComO", 0x2a, Value::Enum { id: 0, version: 0 }),
        ]);
        Some(self.push_node(&[(b"PCrv", 1), (b"VNod", 0), (b"Node", 0)], fields))
    }

    fn clone_subtree(&mut self, index: usize) -> Option<usize> {
        let raw = crate::preserve::preserved_block(
            &self.g,
            &self.g.nodes[index].types,
            b"Node",
            &self.g.nodes[index],
        );
        self.decode_native(&raw)
    }

    fn text_node(&mut self, layer: &Layer, origin: (f64, f64)) -> Option<usize> {
        use schist_text_engine::{Align, ParagraphDirection, TextSpec, WritingMode};
        let stored: serde_json::Value =
            serde_json::from_slice(&layer.extras.iter().find(|b| b.key == *b"PsTx")?.data).ok()?;
        let spec: TextSpec = serde_json::from_value(stored.get("spec")?.clone()).ok()?;
        if spec.text.is_empty()
            || spec.text.len() > 1_000_000
            || spec.path.is_some()
            || spec.writing_mode != WritingMode::Horizontal
            || spec.direction == ParagraphDirection::RightToLeft
            || spec.text.chars().any(|ch| {
                matches!(ch as u32,
                0x0590..=0x08ff | 0x200e..=0x200f | 0x202a..=0x202e |
                0x2066..=0x2069 | 0xfb1d..=0xfdff | 0xfe70..=0xfeff |
                0x10800..=0x10fff | 0x1e800..=0x1eeff)
            })
            || !spec.features.is_empty()
            || spec.tracking != 0.0
            || spec.line_height != 1.0
            || !spec.size.is_finite()
            || !(0.5..=10_000.0).contains(&spec.size)
            || spec.text.contains('\0')
            || spec.text.chars().any(|ch| ch.len_utf16() != 1)
            || spec
                .wrap_width
                .is_some_and(|width| !width.is_finite() || !(1.0..=1_000_000.0).contains(&width))
            || spec.runs.len() > 4096
            || spec.runs.iter().any(|run| {
                run.start > run.end
                    || !spec.text.is_char_boundary(run.start)
                    || !spec.text.is_char_boundary(run.end)
            })
        {
            return None;
        }
        let color: [u8; 4] = serde_json::from_value(stored.get("color")?.clone()).ok()?;
        let pos: [i32; 2] = serde_json::from_value(stored.get("origin")?.clone()).ok()?;
        if spec.runs.iter().any(|run| {
            run.size
                .is_some_and(|size| !size.is_finite() || !(0.5..=10_000.0).contains(&size))
        }) {
            return None;
        }
        let metrics = schist_text_engine::measure(&spec)?;
        // PsTx can originate in an untrusted file. Bound the temporary text
        // mask before allocating it; the layer already has a raster fallback.
        if !metrics.width.is_finite()
            || !metrics.height.is_finite()
            || metrics.width > 1_000_000.0
            || metrics.height > 1_000_000.0
            || (metrics.width as f64 + spec.size as f64 * 4.0)
                * (metrics.height as f64 + spec.size as f64 * 4.0)
                > 16_777_216.0
        {
            return None;
        }
        let raster = schist_text_engine::rasterize(&spec)?;
        let donor = Archive::parse(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/affinity-probe/text_rotated.af"
        )))
        .ok()?;
        let plain = donor.extract(donor.head("doc.dat")?).ok()?;
        let graph = graph::parse(&plain).ok()?;
        let node = graph.nodes.iter().find(|n| n.type_tag() == tag(b"TxtF"))?;
        // Flow is a back-reference to its owning frame. Rebuild it after the
        // node is grafted, rather than recursively expanding that cycle.
        let mut source = Node {
            types: node.types.clone(),
            framing: node.framing,
            chain_end: node.chain_end,
            section_lens: vec![0; node.section_lens.len()],
            ..Node::default()
        };
        for (i, (key, value)) in node.fields.iter().enumerate() {
            if *key == tag(b"Flow") {
                continue;
            }
            source.fields.push((*key, value.clone()));
            source.wire.push(node.wire[i]);
            source.aux.push(node.aux[i]);
        }
        let raw = crate::preserve::preserved_block(&graph, &source.types, b"Node", &source);
        let index = self.decode_native(&raw)?;
        let story = self.find_child(index, b"StSt")?;
        let block = match self.g.nodes[story].field(b"Blok")? {
            Value::Array(items) => match items.first()? {
                Value::Class(Some(i)) => *i,
                _ => return None,
            },
            _ => return None,
        };
        let glyphs = self.find_child(block, b"Glyp")?;
        self.set_field(
            glyphs,
            b"Utf8",
            Value::Str(format!("{}\0", spec.text.replace('\n', "\u{2028}"))),
        );
        let attrs = self.find_child(block, b"GAtt")?;
        let original_run = match self.g.nodes[attrs].field(b"Runs")? {
            Value::Array(items) => match items.first()? {
                Value::Class(Some(i)) => *i,
                _ => return None,
            },
            _ => return None,
        };
        let mut boundaries = vec![0, spec.text.len()];
        for run in &spec.runs {
            if run.start > run.end
                || !spec.text.is_char_boundary(run.start)
                || !spec.text.is_char_boundary(run.end)
            {
                return None;
            }
            boundaries.extend([run.start, run.end]);
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        let mut runs = Vec::new();
        for pair in boundaries.windows(2) {
            let style = spec.style_at(pair[0]);
            if !style.size.is_finite() || style.size <= 0.0 {
                return None;
            }
            let run = self.clone_subtree(original_run)?;
            let item = self.find_child(run, b"Item")?;
            let mut doubles = match self.g.nodes[item].field(b"Doub")? {
                Value::Array(v) => v.clone(),
                _ => return None,
            };
            doubles[0] = Value::F64(style.size as f64);
            self.set_field(item, b"Doub", Value::Array(doubles));
            let post = schist_text_engine::postscript_name(&style.family, style.bold, style.italic)
                .unwrap_or_else(|| style.family.clone());
            for field in [b"DFnt", b"RFnt"] {
                let font = self.find_child(item, field)?;
                for field in [b"Famy", b"Ribi"] {
                    self.set_field(font, field, Value::Str(style.family.clone()));
                }
                self.set_field(font, b"Post", Value::Str(post.clone()));
                self.set_field(
                    font,
                    b"Wegt",
                    Value::I32(if style.bold { 700 } else { 400 }),
                );
                self.set_field(font, b"Ital", Value::Bool(style.italic));
            }
            let rgba = style.color.unwrap_or(color);
            let fill = self.solid_fill(schist_color::Rgba::from_u8(
                rgba[0], rgba[1], rgba[2], rgba[3],
            ));
            let descriptor =
                self.push_node_closed(&[(b"FDsc", 1)], vec![f(b"FDeF", 0x31, Self::class(fill))]);
            let mut objects = match self.g.nodes[item].field(b"Objs")? {
                Value::Array(v) => v.clone(),
                _ => return None,
            };
            objects[0] = Self::class(descriptor);
            self.set_field(item, b"Objs", Value::Array(objects));
            let end = spec.text[..pair[1]].encode_utf16().count()
                + usize::from(pair[1] == spec.text.len());
            self.set_field(run, b"Indx", Value::I32(end as i32));
            runs.push(Self::class(run));
        }
        self.set_field(attrs, b"Runs", Value::Array(runs));
        let paragraphs = self.find_child(block, b"PAtt")?;
        let p_run = match self.g.nodes[paragraphs].field(b"Runs")? {
            Value::Array(items) => match items.first()? {
                Value::Class(Some(i)) => *i,
                _ => return None,
            },
            _ => return None,
        };
        self.set_field(
            p_run,
            b"Indx",
            Value::I32((spec.text.encode_utf16().count() + 1) as i32),
        );
        let paragraph = self.find_child(p_run, b"Item")?;
        let mut ints = match self.g.nodes[paragraph].field(b"Ints")? {
            Value::Array(v) => v.clone(),
            _ => return None,
        };
        ints[0] = Value::I32(match spec.align {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
        });
        self.set_field(paragraph, b"Ints", Value::Array(ints));
        if spec.wrap_width.is_none() {
            // Artistic-frame schema independently observed in the Affinity-
            // written corpus 01_bash_is_bad.afphoto (ArFr v1 < Fram v1).
            self.g.nodes[index].types = vec![(tag(b"TxtA"), 1), (tag(b"Node"), 0)];
            let cap = raster.cap_height.unwrap_or(raster.first_baseline);
            let height =
                cap + spec.text.lines().count().saturating_sub(1) as f32 * raster.line_advance;
            let top = pos[1] as f64 + (raster.first_baseline - cap) as f64;
            let frame = self.push_node_closed(
                &[(b"ArFr", 1), (b"Fram", 1)],
                vec![
                    f(
                        b"FrmB",
                        0x26,
                        Value::VecD(vec![
                            pos[0] as f64,
                            top,
                            pos[0] as f64 + metrics.width as f64,
                            top + height as f64,
                        ]),
                    ),
                    f(b"ArtA", 0x2a, Value::Enum { id: 0, version: 0 }),
                    f(b"ArtV", 0x0a, Value::F64(cap as f64)),
                ],
            );
            self.set_field(index, b"TxtH", Self::class(frame));
            self.replace_field(index, f_aux(b"IgTW", 0x29, 1, Value::Bool(true)));
            self.finish_editable(
                index,
                layer,
                &[],
                origin,
                [1.0, 0.0, -origin.0, 0.0, 1.0, -origin.1],
            );
            return Some(index);
        }
        let frame = self.find_child(index, b"TxtH")?;
        let width = spec.wrap_width.unwrap_or(metrics.width + 1.0).max(1.0) as f64;
        let left = pos[0] as f64
            - match spec.align {
                Align::Left => 0.0,
                Align::Center => (width - raster.layout_width as f64) / 2.0,
                Align::Right => width - raster.layout_width as f64,
            };
        let top = pos[1] as f64 + raster.bounds.top as f64;
        self.set_field(
            frame,
            b"FrmB",
            Value::VecD(vec![
                left,
                top,
                left + width,
                top + metrics.height.max(raster.bounds.height() as f32) as f64,
            ]),
        );
        self.set_field(frame, b"ColW", Value::Array(vec![Value::F64(width)]));
        self.set_field(frame, b"GutW", Value::Array(vec![Value::F64(0.0)]));
        self.finish_editable(
            index,
            layer,
            &[],
            origin,
            [1.0, 0.0, -origin.0, 0.0, 1.0, -origin.1],
        );
        Some(index)
    }
}
