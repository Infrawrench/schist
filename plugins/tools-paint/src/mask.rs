//! One mask tool for brush tips, sharp shapes and persistent gradient handles.
use super::*;
use schist_core::{
    live_mask::{LiveMask, MaskGradient, MaskSnapshot},
    Layer, LayerMask, RawBlock,
};

struct Session {
    original: Layer,
    layer: LayerId,
    before: Option<LayerMask>,
    extras: Vec<RawBlock>,
    active_mask: Option<LayerId>,
    base: LayerMask,
    recipe: LiveMask,
    start: (f32, f32),
    handle: Option<(usize, bool)>,
}

#[derive(Default)]
pub struct MaskTool {
    mode: usize,
    reveal: bool,
    reverse: bool,
    session: Option<Session>,
    stroke: Option<Stroke>,
    cursor: Option<(f32, f32)>,
    selected: Option<(LayerId, usize)>,
    sky_tolerance: f32,
    defer_preview: bool,
    preview_dirty: bool,
}

impl MaskTool {
    fn display(&mut self, doc: &mut Document) {
        if self.defer_preview {
            self.preview_dirty = self.session.is_some();
            return;
        }
        self.preview_dirty = false;
        let Some(s) = &mut self.session else {
            return;
        };
        let mask = s.recipe.render_with_base(&s.base, doc.canvas_rect());
        if let Some(layer) = doc.tree.find_mut(s.layer) {
            layer.mask = Some(mask);
        }
        doc.damage_all();
    }
    fn restore(&mut self, doc: &mut Document) -> Option<Session> {
        self.preview_dirty = false;
        self.stroke = None;
        let session = self.session.take()?;
        if let Some(layer) = doc.tree.find_mut(session.layer) {
            layer.mask = session.before.clone();
            layer.extras = session.extras.clone();
        }
        doc.active_mask = session.active_mask;
        doc.damage_all();
        Some(session)
    }
    fn finish(&mut self, doc: &mut Document) {
        let Some(mut session) = self.restore(doc) else {
            return;
        };
        let Some(layer) = doc.tree.find(session.layer) else {
            return;
        };
        session.recipe.base = MaskSnapshot::capture(&session.base);
        let extras = session.recipe.blocks(layer);
        let Some(mask) = session.recipe.render(doc.canvas_rect()) else {
            return;
        };
        let mut edit = doc.begin_edit(t("tool.mask.name"));
        edit.set_mask(session.layer, Some(mask));
        edit.set_extras(session.layer, extras);
        edit.commit();
        doc.active_mask = Some(session.layer);
    }
}

impl ToolPlugin for MaskTool {
    fn committed_layer(&self) -> Option<&Layer> {
        self.session.as_ref().map(|s| &s.original)
    }
    fn id(&self) -> &'static str {
        "mask"
    }
    fn name(&self) -> &'static str {
        t("tool.mask.name")
    }
    fn description(&self) -> &'static str {
        t("tool.mask.description")
    }
    fn icon(&self) -> &'static str {
        "mask-tool"
    }
    fn group(&self) -> &'static str {
        "brush"
    }
    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, p: PointerInput) {
        self.on_cancel(ctx);
        if !p.x.is_finite() || !p.y.is_finite() || !p.pressure.is_finite() {
            return;
        }
        let Some(layer) = ctx
            .doc
            .active_layer
            .and_then(|id| ctx.doc.tree.find(id))
            .filter(|l| !l.locked && l.mask.as_ref().is_none_or(|m| m.enabled))
        else {
            return;
        };
        let id = layer.id;
        let recipe = LiveMask::from_layer(layer).unwrap_or_else(|| LiveMask {
            base: MaskSnapshot::capture(
                &layer.mask.clone().unwrap_or_else(LayerMask::new_revealing),
            ),
            gradients: Vec::new(),
        });
        let Some(base) = recipe.base.restore() else {
            return;
        };
        let mut session = Session {
            original: layer.clone(),
            layer: id,
            before: layer.mask.clone(),
            extras: layer.extras.clone(),
            active_mask: ctx.doc.active_mask,
            base: base.clone(),
            recipe,
            start: (p.x, p.y),
            handle: None,
        };
        if self.mode == 1 || self.mode == 2 {
            let radius = 8.0 / ctx.state.zoom.max(0.01);
            for (i, g) in session.recipe.gradients.iter().enumerate().rev() {
                if (g.from.0 - p.x).hypot(g.from.1 - p.y) < radius {
                    session.handle = Some((i, false));
                    break;
                }
                if (g.to.0 - p.x).hypot(g.to.1 - p.y) < radius {
                    session.handle = Some((i, true));
                    break;
                }
            }
            if session.handle.is_none() {
                if session.recipe.gradients.len() >= 32 {
                    return;
                }
                let index = session.recipe.gradients.len();
                session.recipe.gradients.push(MaskGradient {
                    from: (p.x, p.y),
                    to: (p.x + 1.0, p.y),
                    radial: self.mode == 2,
                    reverse: self.reverse,
                    opacity: ctx.state.tool_opacity.clamp(0.0, 1.0),
                });
                session.handle = Some((index, true));
            }
            self.selected = session.handle.map(|(i, _)| (id, i));
            if let Some((i, _)) = session.handle {
                self.reverse = session.recipe.gradients[i].reverse;
            }
        }
        if self.mode == 5 {
            let Some(pixels) = ctx
                .doc
                .tree
                .find(id)
                .and_then(|l| l.as_raster())
                .map(|r| r.tiles.clone())
            else {
                return;
            };
            let bounds = pixels.content_bounds().intersect(&ctx.doc.canvas_rect());
            let sky = crate::mask_sky::sky_mask(&pixels, bounds, 0.5 + self.sky_tolerance * 0.5);
            let old = session.base.clone();
            let mut base = sky.clone();
            base.enabled = old.enabled;
            base.linked = old.linked;
            for c in TileCoord::covering(&bounds) {
                let rect = c.rect();
                let clip = rect.intersect(&bounds);
                let tile = base.tiles.get_mut_or_insert(c);
                for y in clip.top..clip.bottom {
                    for x in clip.left..clip.right {
                        let value = if self.reverse {
                            255 - sky.value(x, y)
                        } else {
                            sky.value(x, y)
                        };
                        let cov = ctx.state.tool_opacity * ctx.doc.selection.coverage(x, y) as f32
                            / 255.0;
                        let before = old.value(x, y) as f32;
                        tile[((y - rect.top) * TILE_SIZE + x - rect.left) as usize] =
                            (before + (value as f32 - before) * cov)
                                .round()
                                .clamp(0.0, 255.0) as u8;
                    }
                }
            }
            session.base = base;
            session.recipe.base = MaskSnapshot::capture(&session.base);
            self.session = Some(session);
            self.display(ctx.doc);
            self.finish(ctx.doc);
            return;
        }
        self.session = Some(session);
        ctx.doc.active_mask = Some(id);
        ctx.doc.tree.find_mut(id).unwrap().mask = Some(base);
        if self.mode == 0 {
            let foreground = ctx.state.foreground;
            let reveal = self.reveal ^ p.modifiers.alt;
            ctx.state.foreground = if reveal { Rgba::WHITE } else { Rgba::BLACK };
            self.stroke = Stroke::begin(
                ctx,
                p,
                PaintMode::Brush,
                Ink::Solid(ctx.state.foreground),
                (0, 0),
            );
            ctx.state.foreground = foreground;
            if self.stroke.is_none() {
                self.on_cancel(ctx);
                return;
            }
            self.session.as_mut().unwrap().base =
                ctx.doc.tree.find(id).unwrap().mask.clone().unwrap();
        }
        self.cursor = Some((p.x, p.y));
        self.display(ctx.doc);
    }
    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, p: PointerInput) {
        if !p.x.is_finite() || !p.y.is_finite() || !p.pressure.is_finite() {
            return;
        }
        self.cursor = Some((p.x, p.y));
        let Some(s) = &mut self.session else {
            return;
        };
        if let Some(stroke) = &mut self.stroke {
            ctx.doc.tree.find_mut(s.layer).unwrap().mask = Some(s.base.clone());
            let rotation = stroke.input_rotation(ctx.state.pen_tilt);
            stroke.move_to(ctx.doc, p.x, p.y, p.pressure, rotation);
            s.base = ctx.doc.tree.find(s.layer).unwrap().mask.clone().unwrap();
        } else if let Some((i, to)) = s.handle {
            let g = &mut s.recipe.gradients[i];
            if to {
                g.to = (p.x, p.y);
            } else {
                g.from = (p.x, p.y);
            }
        } else if self.mode == 3 || self.mode == 4 {
            // Analytic antialiasing for sharp mask boundaries. The original
            // base is restored each move, so previews never accumulate.
            let mut base = LiveMask::from_layer(&{
                let mut l = Layer::new_raster("");
                l.extras = s.extras.clone();
                l
            })
            .and_then(|m| m.base.restore())
            .unwrap_or_else(|| s.before.clone().unwrap_or_else(LayerMask::new_revealing));
            let (x0, y0) = (s.start.0.min(p.x), s.start.1.min(p.y));
            let (x1, y1) = (s.start.0.max(p.x), s.start.1.max(p.y));
            let bounds = IntRect::new(
                (x0.floor() as i32).saturating_sub(1),
                (y0.floor() as i32).saturating_sub(1),
                (x1.ceil() as i32).saturating_add(1),
                (y1.ceil() as i32).saturating_add(1),
            )
            .intersect(&ctx.doc.canvas_rect());
            let old = base.clone();
            let expanded = base.bounds.union(&bounds);
            for c in TileCoord::covering(&expanded) {
                let rect = c.rect();
                let tile = base.tiles.get_mut_or_insert(c);
                for y in rect.intersect(&expanded).top..rect.intersect(&expanded).bottom {
                    for x in rect.intersect(&expanded).left..rect.intersect(&expanded).right {
                        let mut cov = 0.0;
                        // Four samples per edge pixel keep ellipse/rectangle edges clean.
                        for (ox, oy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                            let (fx, fy) = (x as f32 + ox, y as f32 + oy);
                            let inside = if self.mode == 3 {
                                fx >= x0 && fx < x1 && fy >= y0 && fy < y1
                            } else {
                                ((fx - (x0 + x1) * 0.5) / ((x1 - x0) * 0.5).max(0.01)).powi(2)
                                    + ((fy - (y0 + y1) * 0.5) / ((y1 - y0) * 0.5).max(0.01)).powi(2)
                                    <= 1.0
                            };
                            if inside {
                                cov += 0.25;
                            }
                        }
                        cov *= ctx.state.tool_opacity * ctx.doc.selection.coverage(x, y) as f32
                            / 255.0;
                        let value = if self.reveal ^ p.modifiers.alt {
                            255.0
                        } else {
                            0.0
                        };
                        let before = old.value(x, y) as f32;
                        tile[((y - rect.top) * TILE_SIZE + x - rect.left) as usize] =
                            (before + (value - before) * cov).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
            base.bounds = expanded;
            s.base = base;
        }
        self.display(ctx.doc);
    }
    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, p: PointerInput) {
        self.on_pointer_move(ctx, p);
        if let (Some(s), Some(stroke)) = (&mut self.session, &mut self.stroke) {
            ctx.doc.tree.find_mut(s.layer).unwrap().mask = Some(s.base.clone());
            stroke.flush(ctx.doc, p, ctx.state.pen_tilt);
            s.base = ctx.doc.tree.find(s.layer).unwrap().mask.clone().unwrap();
        }
        self.display(ctx.doc);
        self.finish(ctx.doc);
    }
    fn on_pointer_move_deferred(&mut self, ctx: &mut ToolCtx, p: PointerInput) {
        // Preserve every brush sample, while composing the live gradients only
        // once per frame. Large masks must not rerender for every input sample.
        self.defer_preview = true;
        self.on_pointer_move(ctx, p);
        self.defer_preview = false;
    }
    fn flush_preview(&mut self, ctx: &mut ToolCtx) -> bool {
        if !self.preview_dirty {
            return false;
        }
        self.display(ctx.doc);
        true
    }
    fn on_cancel(&mut self, ctx: &mut ToolCtx) {
        self.restore(ctx.doc);
    }
    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }
    fn on_document_leave(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
        self.selected = None;
    }
    fn options(&self) -> Vec<ToolOption> {
        let mut options = vec![ToolOption::choice(
            "mask-mode",
            t("common.mode"),
            choices!(&[
                "tool.brush.name",
                "tool.gradient.choice.linear",
                "tool.gradient.choice.radial",
                "tool.shape.rect.name",
                "tool.shape.ellipse.name",
                "tool.mask.sky"
            ]),
            self.mode,
        )];
        if matches!(self.mode, 0 | 3 | 4) {
            options.push(ToolOption::toggle(
                "mask-reveal",
                t("tool.mask.option.reveal"),
                self.reveal,
            ));
        }
        if matches!(self.mode, 1 | 2 | 5) {
            options.push(ToolOption::toggle(
                "mask-reverse",
                t("tool.gradient.option.reverse"),
                self.reverse,
            ));
        }
        if self.mode == 5 {
            options.push(ToolOption::slider(
                "mask-sky-tolerance",
                t("common.tolerance"),
                self.sky_tolerance * 100.0,
                0.0,
                100.0,
                "%",
            ));
        }
        options
    }
    fn set_option(&mut self, key: &str, value: OptionValue) {
        match key {
            "mask-mode" => self.mode = value.index().min(5),
            "mask-sky-tolerance" if value.num().is_finite() => {
                self.sky_tolerance = (value.num() / 100.0).clamp(0.0, 1.0)
            }
            "mask-reveal" => self.reveal = value.bool(),
            "mask-reverse" => self.reverse = value.bool(),
            _ => {}
        }
    }
    fn on_option_changed(&mut self, ctx: &mut ToolCtx, key: &str) {
        if key != "mask-reverse" || !matches!(self.mode, 1 | 2) {
            return;
        }
        let Some(layer) = ctx
            .doc
            .active_layer
            .and_then(|id| ctx.doc.tree.find(id))
            .filter(|l| !l.locked)
        else {
            return;
        };
        let Some(mut recipe) = LiveMask::from_layer(layer) else {
            return;
        };
        let Some(g) = self
            .selected
            .filter(|(id, _)| *id == layer.id)
            .and_then(|(_, i)| recipe.gradients.get_mut(i))
        else {
            return;
        };
        g.reverse = self.reverse;
        let id = layer.id;
        let extras = recipe.blocks(layer);
        if let Some(mask) = recipe.render(ctx.doc.canvas_rect()) {
            let mut edit = ctx.doc.begin_edit(t("tool.mask.name"));
            edit.set_mask(id, Some(mask));
            edit.set_extras(id, extras);
            edit.commit();
        }
    }
    fn on_key(
        &mut self,
        ctx: &mut ToolCtx,
        key: &str,
        _text: Option<&str>,
        _mods: schist_plugin_api::Modifiers,
    ) -> bool {
        if (key == "delete" || key == "backspace") && (self.mode == 1 || self.mode == 2) {
            let Some(layer) = ctx
                .doc
                .active_layer
                .and_then(|id| ctx.doc.tree.find(id))
                .filter(|l| !l.locked)
            else {
                return false;
            };
            let Some(mut recipe) = LiveMask::from_layer(layer) else {
                return false;
            };
            let Some((_, i)) = self
                .selected
                .filter(|&(id, i)| id == layer.id && i < recipe.gradients.len())
            else {
                return false;
            };
            recipe.gradients.remove(i);
            let id = layer.id;
            let extras = recipe.blocks(layer);
            if let Some(mask) = recipe.render(ctx.doc.canvas_rect()) {
                let mut edit = ctx.doc.begin_edit(t("tool.mask.name"));
                edit.set_mask(id, Some(mask));
                edit.set_extras(id, extras);
                edit.commit();
            }
            self.selected = None;
            return true;
        }
        false
    }
    fn overlays(&self, doc: &Document, state: &EditorState) -> Vec<Overlay> {
        let mut out = Vec::new();
        let saved = doc
            .active_layer
            .and_then(|id| doc.tree.find(id))
            .and_then(LiveMask::from_layer);
        if let Some(recipe) = self.session.as_ref().map(|s| &s.recipe).or(saved.as_ref()) {
            for g in &recipe.gradients {
                out.push(Overlay::GuideLine {
                    x1: g.from.0,
                    y1: g.from.1,
                    x2: g.to.0,
                    y2: g.to.1,
                });
                for (x, y) in [g.from, g.to] {
                    out.push(Overlay::Circle {
                        cx: x,
                        cy: y,
                        r: 5.0 / state.zoom.max(0.01),
                    });
                }
            }
        }
        if self.mode == 0 {
            if let Some((x, y)) = self.cursor {
                out.push(Overlay::Circle {
                    cx: x,
                    cy: y,
                    r: state.brush_size * 0.5,
                });
            }
        }
        out
    }
}
