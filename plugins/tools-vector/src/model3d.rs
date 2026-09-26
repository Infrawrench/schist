use schist_core::{
    model3d::{Model3d, Placement},
    Document, Layer, LayerId, TileMap,
};
use schist_i18n::{choices, t};
use schist_plugin_api::{OptionValue, PointerInput, ToolCtx, ToolOption, ToolPlugin};

#[derive(Default)]
pub struct ModelTool {
    mode: usize,
    settings: Option<Placement>,
    session: Option<(Layer, Model3d, (f32, f32))>,
    pending: Option<Placement>,
    origin: Option<Placement>,
    synced: Option<(schist_core::DocumentId, Option<LayerId>, u64)>,
}
fn selected(doc: &Document) -> Option<(LayerId, Model3d)> {
    let layer = doc
        .active_layer
        .and_then(|id| doc.tree.find(id))
        .filter(|l| !l.locked)?;
    Some((layer.id, Model3d::from_layer(layer)?))
}
fn apply(doc: &mut Document, id: LayerId, model: &Model3d) -> bool {
    let Ok(tiles) = schist_model3d::render(model, doc.depth, doc.canvas_rect()) else {
        return false;
    };
    let Some(layer) = doc.tree.find(id).filter(|l| !l.locked) else {
        return false;
    };
    let extras = model.blocks(layer);
    let mut edit = doc.begin_edit(t("tool.model3d.name"));
    edit.set_extras(id, extras);
    edit.replace_layer_render(id, tiles);
    edit.commit();
    true
}
impl ModelTool {
    fn preview(&mut self, ctx: &mut ToolCtx) -> bool {
        let Some(placement) = self.pending.take() else {
            return false;
        };
        let Some((before, model, _)) = &mut self.session else {
            return false;
        };
        model.placement = placement.clone();
        let Ok(tiles) = schist_model3d::render(model, ctx.doc.depth, ctx.doc.canvas_rect()) else {
            return false;
        };
        if let Some(layer) = ctx.doc.tree.find_mut(before.id) {
            layer.as_raster_mut().unwrap().tiles = tiles;
            layer.styled = None;
        }
        self.settings = Some(placement);
        ctx.doc.damage_all();
        true
    }
    fn drag(&mut self, p: PointerInput) {
        if !p.x.is_finite() || !p.y.is_finite() {
            return;
        }
        let Some((_, _, start)) = &self.session else {
            return;
        };
        let Some(mut placement) = self.origin.clone() else {
            return;
        };
        let (dx, dy) = (p.x - start.0, p.y - start.1);
        match self.mode {
            1 => {
                placement.transform.tx += dx;
                placement.transform.ty += dy;
            }
            2 => placement.scale = (placement.scale * (-dy * 0.01).exp()).clamp(0.1, 4096.0),
            3 => {
                placement.light_azimuth += dx * 0.5;
                placement.light_elevation =
                    (placement.light_elevation - dy * 0.5).clamp(-89.0, 89.0);
            }
            _ => {
                placement.rotation[0] += dy * 0.5;
                placement.rotation[1] += dx * 0.5;
            }
        }
        self.pending = Some(placement);
    }
}
impl ToolPlugin for ModelTool {
    fn id(&self) -> &'static str {
        "model3d"
    }
    fn name(&self) -> &'static str {
        t("tool.model3d.name")
    }
    fn description(&self) -> &'static str {
        t("tool.model3d.description")
    }
    fn icon(&self) -> &'static str {
        "model3d"
    }
    fn committed_layer_pixels(&self) -> Option<(LayerId, &TileMap)> {
        let (layer, _, _) = self.session.as_ref()?;
        Some((layer.id, &layer.as_raster()?.tiles))
    }
    fn on_activate(&mut self, ctx: &mut ToolCtx) {
        self.settings = selected(ctx.doc).map(|(_, m)| m.placement);
    }
    fn sync_document(&mut self, doc: &Document) {
        let key = (doc.id, doc.active_layer, doc.revision);
        if self.session.is_none() && self.synced != Some(key) {
            self.settings = selected(doc).map(|(_, m)| m.placement);
            self.synced = Some(key);
        }
    }
    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, p: PointerInput) {
        self.on_cancel(ctx);
        if !p.x.is_finite() || !p.y.is_finite() {
            return;
        }
        if let Some((id, model)) = selected(ctx.doc) {
            self.origin = Some(model.placement.clone());
            self.settings = Some(model.placement.clone());
            self.session = Some((ctx.doc.tree.find(id).unwrap().clone(), model, (p.x, p.y)));
        }
    }
    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, p: PointerInput) {
        self.drag(p);
        self.preview(ctx);
    }
    fn on_pointer_move_deferred(&mut self, _ctx: &mut ToolCtx, p: PointerInput) {
        self.drag(p);
    }
    fn flush_preview(&mut self, ctx: &mut ToolCtx) -> bool {
        self.preview(ctx)
    }
    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, p: PointerInput) {
        self.on_pointer_move(ctx, p);
        if let Some((before, mut model, _)) = self.session.take() {
            let id = before.id;
            if let Some(placement) = self.settings.clone() {
                model.placement = placement;
            }
            if let Some(layer) = ctx.doc.tree.find_mut(id) {
                *layer = before;
            }
            apply(ctx.doc, id, &model);
        }
    }
    fn on_cancel(&mut self, ctx: &mut ToolCtx) {
        self.pending = None;
        if let Some((before, _, _)) = self.session.take() {
            if let Some(layer) = ctx.doc.tree.find_mut(before.id) {
                *layer = before;
            }
            ctx.doc.damage_all();
            self.settings = selected(ctx.doc).map(|(_, m)| m.placement);
        }
    }
    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }
    fn on_document_leave(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }
    fn options(&self) -> Vec<ToolOption> {
        let p = self
            .settings
            .clone()
            .unwrap_or_else(|| Placement::fitted(1024, 1024));
        vec![
            ToolOption::choice(
                "model-mode",
                t("common.mode"),
                choices!(&[
                    "tool.model3d.rotate",
                    "tool.model3d.move",
                    "tool.model3d.scale",
                    "tool.model3d.light"
                ]),
                self.mode,
            ),
            ToolOption::slider(
                "model-x",
                t("tool.model3d.x"),
                p.rotation[0],
                -180.0,
                180.0,
                "°",
            ),
            ToolOption::slider(
                "model-y",
                t("tool.model3d.y"),
                p.rotation[1],
                -180.0,
                180.0,
                "°",
            ),
            ToolOption::slider(
                "model-z",
                t("tool.model3d.z"),
                p.rotation[2],
                -180.0,
                180.0,
                "°",
            ),
            ToolOption::slider(
                "model-scale",
                t("tool.model3d.scale"),
                p.scale,
                1.0,
                4096.0,
                " px",
            ),
            ToolOption::slider(
                "model-azimuth",
                t("tool.model3d.azimuth"),
                p.light_azimuth,
                -180.0,
                180.0,
                "°",
            ),
            ToolOption::slider(
                "model-elevation",
                t("tool.model3d.elevation"),
                p.light_elevation,
                -89.0,
                89.0,
                "°",
            ),
            ToolOption::slider(
                "model-intensity",
                t("tool.model3d.intensity"),
                p.light_intensity,
                0.0,
                4.0,
                "",
            ),
            ToolOption::slider(
                "model-ambient",
                t("tool.model3d.ambient"),
                p.ambient,
                0.0,
                1.0,
                "",
            ),
            ToolOption::toggle(
                "model-perspective",
                t("tool.model3d.perspective"),
                p.perspective,
            ),
        ]
    }
    fn set_option(&mut self, key: &str, value: OptionValue) {
        if key == "model-mode" {
            self.mode = value.index().min(3);
            return;
        }
        if !value.num().is_finite() {
            return;
        }
        if let Some(p) = &mut self.settings {
            match key {
                "model-x" => p.rotation[0] = value.num().clamp(-180.0, 180.0),
                "model-y" => p.rotation[1] = value.num().clamp(-180.0, 180.0),
                "model-z" => p.rotation[2] = value.num().clamp(-180.0, 180.0),
                "model-scale" => p.scale = value.num().clamp(1.0, 4096.0),
                "model-azimuth" => p.light_azimuth = value.num().clamp(-180.0, 180.0),
                "model-elevation" => p.light_elevation = value.num().clamp(-89.0, 89.0),
                "model-intensity" => p.light_intensity = value.num().clamp(0.0, 4.0),
                "model-ambient" => p.ambient = value.num().clamp(0.0, 1.0),
                "model-perspective" => p.perspective = value.bool(),
                _ => {}
            }
        }
    }
    fn on_option_changed(&mut self, ctx: &mut ToolCtx, key: &str) {
        if key == "model-mode" {
            return;
        }
        if let (Some((id, mut model)), Some(settings)) = (selected(ctx.doc), self.settings.clone())
        {
            model.placement = settings;
            apply(ctx.doc, id, &model);
        }
    }
}
