//! Blend creation and direct editing, through the same tool API as the GUI/MCP.
use schist_core::{
    curves,
    vector_blend::{Easing, VectorBlend},
    Anchor, Document, Layer, LayerId, LayerPath, SubPath, VectorPath,
};
use schist_i18n::{choices, t};
use schist_plugin_api::{
    EditorState, Modifiers, OptionValue, Overlay, PointerInput, ToolCtx, ToolOption, ToolPlugin,
};

pub fn render(blend: &VectorBlend, doc: &Document) -> schist_core::TileMap {
    let mut tiles = schist_core::TileMap::new();
    for shape in blend.shapes() {
        let flat = crate::paths::flatten(&shape.path);
        crate::rasterize_into(
            &mut tiles,
            &flat,
            if shape.even_odd {
                schist_vector::FillRule::EvenOdd
            } else {
                schist_vector::FillRule::NonZero
            },
            shape.fill,
            doc.depth,
            doc.canvas_rect(),
        );
        if let Some((color, width)) = shape.stroke {
            let stroke = schist_vector::stroke_path(&flat, schist_vector::StrokeStyle::new(width));
            crate::rasterize_into(
                &mut tiles,
                &stroke,
                schist_vector::FillRule::NonZero,
                color,
                doc.depth,
                doc.canvas_rect(),
            );
        }
    }
    tiles
}

pub fn create(doc: &mut Document, a: LayerId, b: LayerId) -> Option<LayerId> {
    if a == b {
        return None;
    }
    let left = doc.tree.find(a)?;
    let right = doc.tree.find(b)?;
    if left.locked || right.locked {
        return None;
    }
    let blend = VectorBlend::new(*left.shape.clone()?, *right.shape.clone()?);
    let mut layer = Layer::new_raster(t("tool.vector_blend.name"));
    layer.extras = blend.blocks(&layer);
    layer.as_raster_mut()?.tiles = render(&blend, doc);
    let id = layer.id;
    let path = LayerPath(vec![doc.tree.layers.len()]);
    let mut edit = doc.begin_edit(t("tool.vector_blend.name"));
    edit.change_props(a, |l| l.visible = false);
    edit.change_props(b, |l| l.visible = false);
    edit.insert_layer(path, layer);
    edit.commit();
    doc.active_layer = Some(id);
    Some(id)
}

fn apply(doc: &mut Document, id: LayerId, blend: &VectorBlend) {
    let Some(layer) = doc.tree.find(id).filter(|l| !l.locked) else {
        return;
    };
    if !blend.valid() {
        return;
    }
    let extras = blend.blocks(layer);
    let tiles = render(blend, doc);
    let mut edit = doc.begin_edit(t("tool.vector_blend.name"));
    edit.set_extras(id, extras);
    edit.replace_layer_render(id, tiles);
    edit.commit();
}

#[derive(Default)]
pub struct BlendTool {
    first: Option<LayerId>,
    mode: usize,
    settings: Option<VectorBlend>,
    before: Option<Layer>,
    grabbed: Option<(bool, usize, usize)>,
    map_start: Option<(usize, usize)>,
    trace: Vec<(f32, f32)>,
    guide_grab: Option<(usize, usize)>,
    synced: Option<(schist_core::DocumentId, Option<LayerId>, u64)>,
    defer_preview: bool,
    pending_preview: Option<(LayerId, VectorBlend)>,
}

impl BlendTool {
    fn current(doc: &Document) -> Option<(LayerId, VectorBlend)> {
        let layer = doc.tree.find(doc.active_layer?)?;
        if layer.locked {
            return None;
        }
        Some((layer.id, VectorBlend::from_layer(layer)?))
    }
    fn preview(&mut self, doc: &mut Document, id: LayerId, blend: VectorBlend) {
        if self.defer_preview {
            self.pending_preview = Some((id, blend));
            return;
        }
        self.pending_preview = None;
        let tiles = render(&blend, doc);
        if let Some(l) = doc.tree.find_mut(id) {
            l.extras = blend.blocks(l);
            l.as_raster_mut().unwrap().tiles = tiles;
            l.styled = None;
        }
        self.settings = Some(blend);
        doc.damage_all();
    }
    fn hit(blend: &VectorBlend, p: PointerInput, r: f32) -> Option<(bool, usize, usize)> {
        for (end, path) in [(false, &blend.start.path), (true, &blend.end.path)] {
            if let Some((s, a)) = path.hit_anchor(p.x, p.y, r) {
                return Some((end, s, a));
            }
        }
        None
    }
}

impl ToolPlugin for BlendTool {
    fn committed_layer(&self) -> Option<&Layer> {
        self.before.as_ref()
    }
    fn id(&self) -> &'static str {
        "vector_blend"
    }
    fn name(&self) -> &'static str {
        t("tool.vector_blend.name")
    }
    fn description(&self) -> &'static str {
        t("tool.vector_blend.description")
    }
    fn icon(&self) -> &'static str {
        "blend-tool"
    }
    fn group(&self) -> &'static str {
        "shape"
    }
    fn on_activate(&mut self, ctx: &mut ToolCtx) {
        self.settings = Self::current(ctx.doc).map(|(_, b)| b);
        self.first = None;
    }
    fn sync_document(&mut self, doc: &Document) {
        let key = (doc.id, doc.active_layer, doc.revision);
        if self.before.is_none() && self.synced != Some(key) {
            self.settings = Self::current(doc).map(|(_, b)| b);
            self.synced = Some(key);
        }
    }
    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, p: PointerInput) {
        self.on_cancel(ctx);
        if !p.x.is_finite() || !p.y.is_finite() {
            return;
        }
        if let Some((id, mut blend)) = Self::current(ctx.doc) {
            self.settings = Some(blend.clone());
            if self.mode == 3 {
                if let Some((end, s, a)) = Self::hit(&blend, p, 10.0 / ctx.state.zoom.max(0.01)) {
                    if !end {
                        self.map_start = Some((s, a));
                    } else if let Some((ls, la)) = self.map_start.take().filter(|(ls, _)| *ls == s)
                    {
                        let pairs = schist_core::vector_blend::paired_paths(
                            &blend.start.path,
                            &blend.end.path,
                            &blend.mapping,
                        );
                        if let Some((left, right)) = pairs.get(ls) {
                            if la < left.anchors.len() && a < right.anchors.len() {
                                let source = left
                                    .anchors
                                    .iter()
                                    .position(|an| {
                                        an.point == blend.start.path.subpaths[ls].anchors[la].point
                                    })
                                    .unwrap_or(la);
                                let target = right
                                    .anchors
                                    .iter()
                                    .position(|an| {
                                        an.point == blend.end.path.subpaths[ls].anchors[a].point
                                    })
                                    .unwrap_or(a);
                                // Freeze automatic correspondence, then swap a pair to
                                // keep the explicit mapping a complete permutation.
                                blend.start.path.subpaths =
                                    pairs.iter().map(|p| p.0.clone()).collect();
                                blend.end.path.subpaths =
                                    pairs.iter().map(|p| p.1.clone()).collect();
                                blend.mapping = pairs
                                    .iter()
                                    .map(|p| (0..p.0.anchors.len()).collect())
                                    .collect();
                                blend.mapping[ls].swap(source, target);
                                apply(ctx.doc, id, &blend);
                                self.settings = Some(blend);
                            }
                        }
                    }
                }
                return;
            }
            if self.mode == 1 || self.mode == 2 {
                self.before = ctx.doc.tree.find(id).cloned();
                let guide = if self.mode == 1 {
                    blend.spine.as_ref()
                } else {
                    blend.rail.as_ref()
                };
                self.guide_grab = guide
                    .and_then(|path| path.hit_anchor(p.x, p.y, 10.0 / ctx.state.zoom.max(0.01)));
                if self.guide_grab.is_none() {
                    self.trace = vec![(p.x, p.y)];
                }
            } else if let Some(hit) = Self::hit(&blend, p, 10.0 / ctx.state.zoom.max(0.01)) {
                self.before = ctx.doc.tree.find(id).cloned();
                self.grabbed = Some(hit);
            }
            return;
        }
        let hit = ctx
            .doc
            .tree
            .iter()
            .filter(|l| l.visible && !l.locked && l.shape.is_some())
            .filter(|l| {
                l.shape
                    .as_ref()
                    .unwrap()
                    .path
                    .bounds()
                    .contains(p.x as i32, p.y as i32)
            })
            .last()
            .map(|l| l.id);
        if let Some(id) = hit {
            if let Some(first) = self.first.take().filter(|first| *first != id) {
                if create(ctx.doc, first, id).is_some() {
                    self.settings = Self::current(ctx.doc).map(|(_, b)| b);
                }
            } else {
                self.first = Some(id);
            }
        }
    }
    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, p: PointerInput) {
        if !p.x.is_finite() || !p.y.is_finite() {
            return;
        }
        let Some(before) = self.before.as_ref() else {
            return;
        };
        let id = before.id;
        let Some(mut blend) = VectorBlend::from_layer(before) else {
            return;
        };
        if let Some((end, s, a)) = self.grabbed {
            let path = if end {
                &mut blend.end.path
            } else {
                &mut blend.start.path
            };
            let anchor = &mut path.subpaths[s].anchors[a];
            if p.modifiers.shift {
                let delta = (p.x - anchor.point.0, p.y - anchor.point.1);
                path.translate(delta.0, delta.1);
                self.preview(ctx.doc, id, blend);
                return;
            }
            if p.modifiers.alt {
                anchor.handle_out = (p.x - anchor.point.0, p.y - anchor.point.1);
                anchor.handle_in = (-anchor.handle_out.0, -anchor.handle_out.1);
            } else {
                anchor.point = (p.x, p.y);
            }
        } else if let Some((s, a)) = self.guide_grab {
            let guide = if self.mode == 1 {
                blend.spine.as_mut()
            } else {
                blend.rail.as_mut()
            };
            if let Some(path) = guide {
                let anchor = &mut path.subpaths[s].anchors[a];
                if p.modifiers.shift {
                    let delta = (p.x - anchor.point.0, p.y - anchor.point.1);
                    path.translate(delta.0, delta.1);
                } else if p.modifiers.alt {
                    anchor.handle_out = (p.x - anchor.point.0, p.y - anchor.point.1);
                    anchor.handle_in = (-anchor.handle_out.0, -anchor.handle_out.1);
                } else {
                    anchor.point = (p.x, p.y);
                }
            }
        } else if !self.trace.is_empty() {
            if self.trace.last().is_some_and(|last| {
                curves::length(curves::sub(*last, (p.x, p.y))) >= 3.0 / ctx.state.zoom.max(0.01)
            }) && self.trace.len() < 1024
            {
                self.trace.push((p.x, p.y));
            }
            let mut path = VectorPath::new(String::new());
            path.subpaths.push(SubPath {
                closed: false,
                anchors: self
                    .trace
                    .iter()
                    .map(|&(x, y)| Anchor::corner(x, y))
                    .collect(),
            });
            path.smooth_all();
            if self.mode == 1 {
                blend.spine = Some(path);
            } else {
                if blend.spine.is_none() {
                    let center = |s: &schist_core::VectorShape| {
                        let b = s.path.bounds();
                        Anchor::corner(
                            (b.left + b.right) as f32 * 0.5,
                            (b.top + b.bottom) as f32 * 0.5,
                        )
                    };
                    let mut spine = VectorPath::new(String::new());
                    spine.push_open_anchors(vec![center(&blend.start), center(&blend.end)]);
                    blend.spine = Some(spine);
                }
                blend.rail = Some(path);
            }
        }
        self.preview(ctx.doc, id, blend);
    }
    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, p: PointerInput) {
        self.on_pointer_move(ctx, p);
        if let Some(before) = self.before.take() {
            let after = ctx
                .doc
                .tree
                .find(before.id)
                .and_then(VectorBlend::from_layer);
            let id = before.id;
            if let Some(l) = ctx.doc.tree.find_mut(id) {
                *l = before;
            }
            if let Some(after) = after {
                apply(ctx.doc, id, &after);
            }
        }
        self.grabbed = None;
        self.guide_grab = None;
        self.trace.clear();
    }
    fn on_pointer_move_deferred(&mut self, ctx: &mut ToolCtx, p: PointerInput) {
        self.defer_preview = true;
        self.on_pointer_move(ctx, p);
        self.defer_preview = false;
    }
    fn flush_preview(&mut self, ctx: &mut ToolCtx) -> bool {
        let Some((id, blend)) = self.pending_preview.take() else {
            return false;
        };
        self.preview(ctx.doc, id, blend);
        true
    }
    fn on_cancel(&mut self, ctx: &mut ToolCtx) {
        self.pending_preview = None;
        if let Some(before) = self.before.take() {
            if let Some(l) = ctx.doc.tree.find_mut(before.id) {
                *l = before;
            }
            ctx.doc.damage_all();
            self.settings = Self::current(ctx.doc).map(|(_, b)| b);
        }
        self.grabbed = None;
        self.guide_grab = None;
        self.trace.clear();
    }
    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
        self.first = None;
    }
    fn on_document_leave(&mut self, ctx: &mut ToolCtx) {
        self.on_deactivate(ctx);
        self.map_start = None;
    }
    fn on_key(
        &mut self,
        ctx: &mut ToolCtx,
        key: &str,
        _text: Option<&str>,
        _modifiers: Modifiers,
    ) -> bool {
        if key == "delete" || key == "backspace" {
            if let Some((id, mut blend)) = Self::current(ctx.doc) {
                match self.mode {
                    1 => {
                        blend.spine = None;
                        blend.rail = None;
                    }
                    2 => blend.rail = None,
                    3 => blend.mapping.clear(),
                    _ => return false,
                }
                apply(ctx.doc, id, &blend);
                self.settings = Some(blend);
                return true;
            }
        }
        false
    }
    fn options(&self) -> Vec<ToolOption> {
        let (steps, bias, easing, orient) = self
            .settings
            .as_ref()
            .map(|b| (b.steps, b.bias, b.easing, b.orient))
            .unwrap_or((12, 0.5, Easing::Linear, false));
        vec![
            ToolOption::choice(
                "blend-mode",
                t("common.mode"),
                choices!(&[
                    "tool.vector_blend.mode.shapes",
                    "tool.vector_blend.mode.spine",
                    "tool.vector_blend.mode.rail",
                    "tool.vector_blend.mode.mapping"
                ]),
                self.mode,
            ),
            ToolOption::slider(
                "blend-steps",
                t("tool.vector_blend.option.steps"),
                steps as f32,
                0.0,
                1024.0,
                "",
            ),
            ToolOption::slider(
                "blend-bias",
                t("tool.vector_blend.option.bias"),
                bias * 100.0,
                1.0,
                99.0,
                "%",
            ),
            ToolOption::choice(
                "blend-easing",
                t("tool.vector_blend.option.easing"),
                choices!(&[
                    "tool.vector_blend.easing.linear",
                    "tool.vector_blend.easing.in",
                    "tool.vector_blend.easing.out",
                    "tool.vector_blend.easing.both"
                ]),
                match easing {
                    Easing::Linear => 0,
                    Easing::EaseIn => 1,
                    Easing::EaseOut => 2,
                    Easing::EaseInOut => 3,
                },
            ),
            ToolOption::toggle("blend-orient", t("tool.vector_blend.option.orient"), orient),
        ]
    }
    fn set_option(&mut self, key: &str, value: OptionValue) {
        if key == "blend-mode" {
            self.mode = value.index().min(3);
            return;
        }
        if !value.num().is_finite() {
            return;
        }
        if let Some(b) = &mut self.settings {
            match key {
                "blend-steps" => b.steps = value.index().min(1024),
                "blend-bias" => b.bias = (value.num() / 100.0).clamp(0.01, 0.99),
                "blend-easing" => {
                    b.easing = match value.index() {
                        1 => Easing::EaseIn,
                        2 => Easing::EaseOut,
                        3 => Easing::EaseInOut,
                        _ => Easing::Linear,
                    }
                }
                "blend-orient" => b.orient = value.bool(),
                _ => {}
            }
        }
    }
    fn on_option_changed(&mut self, ctx: &mut ToolCtx, key: &str) {
        if key == "blend-mode" {
            self.map_start = None;
            return;
        }
        if let (Some((id, _)), Some(b)) = (Self::current(ctx.doc), &self.settings) {
            apply(ctx.doc, id, b);
        }
    }
    fn overlays(&self, doc: &Document, state: &EditorState) -> Vec<Overlay> {
        let mut out = Vec::new();
        if let Some((_, b)) = Self::current(doc) {
            for p in [&b.start.path, &b.end.path]
                .into_iter()
                .chain(b.spine.iter())
                .chain(b.rail.iter())
            {
                for sub in crate::paths::flatten(p).subpaths {
                    out.push(Overlay::AntsPolygon(sub));
                }
                for (_, _, a) in p.anchors() {
                    out.push(Overlay::Circle {
                        cx: a.point.0,
                        cy: a.point.1,
                        r: 4.0 / state.zoom.max(0.01),
                    });
                }
            }
            if self.mode == 3 {
                for (a, b) in
                    schist_core::vector_blend::paired_paths(&b.start.path, &b.end.path, &b.mapping)
                {
                    for (a, b) in a.anchors.iter().zip(&b.anchors) {
                        out.push(Overlay::Line {
                            x1: a.point.0,
                            y1: a.point.1,
                            x2: b.point.0,
                            y2: b.point.1,
                        });
                    }
                }
            }
        } else if let Some(l) = self.first.and_then(|id| doc.tree.find(id)) {
            out.push(Overlay::Rect(l.content_bounds()));
        }
        out
    }
}
