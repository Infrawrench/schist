//! Layer filter stacks: source-space rendering, transactional preview and commit.
use super::*;
use crate::ui;
use schist_core::filter_stack::{self, FilterEffect, FilterStack};
use schist_core::LayerId;
use schist_i18n::{t, tf};
use schist_plugin_api::FilterValues;

#[derive(Clone)]
pub struct StackFilterSession {
    layer: LayerId,
    original: Arc<Layer>,
    source: schist_core::TileMap,
    stack: FilterStack,
    index: usize,
}

#[derive(Clone, Copy)]
pub enum StackChange {
    Toggle(usize),
    Up(usize),
    Down(usize),
    Remove(usize),
    Bake,
}

fn source_and_stack(
    layer: &Layer,
    canvas: IntRect,
) -> anyhow::Result<(schist_core::TileMap, FilterStack)> {
    if let Some(stack) = FilterStack::read(layer)? {
        return Ok((filter_stack::read_source(layer)?, stack));
    }
    anyhow::ensure!(!filter_stack::has_stack(layer), "Incomplete filter stack");
    let raster = layer
        .as_raster()
        .ok_or_else(|| anyhow::anyhow!("Filter stacks need a pixel layer"))?;
    let (source, region) = if let Some(smart) = layer.smart.as_deref() {
        (smart.source.clone(), smart.source_bounds)
    } else {
        (
            raster.tiles.clone(),
            raster.tiles.tile_bounds().intersect(&canvas),
        )
    };
    let stack = FilterStack::new(region);
    stack.validate()?;
    Ok((source, stack))
}

/// Keeps smart objects editable: effects run on their source artwork before the
/// placement transform, and the unfiltered source remains in the private block.
fn render_layer(
    registry: &schist_plugin_api::registry::PluginRegistry,
    layer: &Layer,
    source: &schist_core::TileMap,
    stack: &FilterStack,
    doc: &Document,
) -> anyhow::Result<(schist_core::TileMap, Option<Box<schist_core::SmartObject>>)> {
    let filtered = schist_plugin_api::filter_stack::render(
        registry,
        stack,
        source,
        doc.depth,
        doc.icc_profile.clone(),
    )?;
    if let Some(mut smart) = layer.smart.clone() {
        smart.source = filtered;
        smart.source_bounds = smart.source.content_bounds();
        let tiles = smart.render(doc.depth, doc.canvas_rect());
        Ok((tiles, Some(smart)))
    } else {
        Ok((filtered, None))
    }
}

/// Full persisted document state, excluding undo history and display caches.
/// Replacing the one previewed layer makes recovery independent of modal state.
fn committed_snapshot(doc: &Document, original: &Layer) -> Document {
    let mut saved = Document::new(doc.title.clone(), doc.width, doc.height, doc.depth);
    saved.id = doc.id;
    saved.path = doc.path.clone();
    saved.resolution_dpi = doc.resolution_dpi;
    saved.mode = doc.mode;
    saved.icc_profile = doc.icc_profile.clone();
    saved.tree = doc.tree.clone();
    saved.selection = doc.selection.clone();
    saved.active_layer = doc.active_layer;
    saved.selected = doc.selected.clone();
    saved.preserved_resources = doc.preserved_resources.clone();
    saved.global_layer_mask = doc.global_layer_mask.clone();
    saved.preserved_layer_info = doc.preserved_layer_info.clone();
    saved.revision = doc.revision;
    saved.guides = doc.guides.clone();
    saved.artboards = doc.artboards.clone();
    saved.slices = doc.slices.clone();
    saved.notes = doc.notes.clone();
    saved.counts = doc.counts.clone();
    saved.layer_comps = doc.layer_comps.clone();
    saved.paths = doc.paths.clone();
    saved.active_path = doc.active_path;
    saved.dirty = doc.dirty;
    if let Some(layer) = saved.tree.find_mut(original.id) {
        *layer = original.clone();
    }
    saved
}

impl Workspace {
    /// A tab switch can arrive from a keyboard shortcut while the dialog is up.
    pub(super) fn discard_stack_filter_for_document_change(&mut self) {
        if let Some(session) = self.stack_filter_session.take() {
            if let Some(layer) = self
                .doc
                .as_mut()
                .and_then(|doc| doc.tree.find_mut(session.layer))
            {
                *layer = (*session.original).clone();
            }
            self.modal = None;
            self.modal_stack.clear();
        }
    }

    pub(super) fn filter_stack_saved_document(&self, doc: &Document) -> Option<Document> {
        let session = self.stack_filter_session.as_ref()?;
        if self.doc.as_ref()?.id != doc.id {
            return None;
        }
        Some(committed_snapshot(doc, &session.original))
    }

    pub fn open_stack_filter(
        &mut self,
        id: &'static str,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let result = (|| -> anyhow::Result<_> {
            let doc = self
                .doc
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No document"))?;
            let layer = doc
                .active_layer
                .and_then(|id| doc.tree.find(id))
                .ok_or_else(|| anyhow::anyhow!("No active layer"))?;
            anyhow::ensure!(
                !layer.locked
                    && layer.shape.is_none()
                    && !layer.extras.iter().any(|b| b.key == *b"PsTx"),
                "Use an unlocked pixel layer or smart object"
            );
            let filter = self
                .registry
                .shared_filter(id)
                .ok_or_else(|| anyhow::anyhow!("Unavailable filter"))?;
            anyhow::ensure!(
                schist_plugin_api::filter_stack::eligible(filter.as_ref()),
                "Filter needs external inputs"
            );
            let (source, mut stack) = source_and_stack(layer, doc.canvas_rect())?;
            let values = if let Some(index) = index {
                schist_plugin_api::filter_stack::values(
                    filter.as_ref(),
                    stack
                        .effects
                        .get(index)
                        .ok_or_else(|| anyhow::anyhow!("Missing effect"))?,
                )?
            } else {
                FilterValues::defaults(&filter.params())
            };
            let index = index.unwrap_or(stack.effects.len());
            if index == stack.effects.len() {
                let fg = self.editor.foreground;
                let bg = self.editor.background;
                stack.effects.push(FilterEffect {
                    id: id.into(),
                    enabled: true,
                    values: values.0.iter().map(|(k, v)| ((*k).into(), *v)).collect(),
                    foreground: [fg.r, fg.g, fg.b, fg.a],
                    background: [bg.r, bg.g, bg.b, bg.a],
                });
            }
            stack.validate()?;
            Ok((
                StackFilterSession {
                    layer: layer.id,
                    original: Arc::new(layer.clone()),
                    source,
                    stack,
                    index,
                },
                values,
            ))
        })();
        match result {
            Ok((session, values)) => {
                self.filter_stack_picker = false;
                self.stack_filter_session = Some(session);
                self.open_modal(
                    Modal::Filter {
                        id,
                        values: values.clone(),
                        preview: true,
                        map: None,
                    },
                    cx,
                );
                self.preview_stack_filter(Some(&values), cx);
            }
            Err(error) => {
                log::warn!("filter stack: {error:#}");
                self.status = t("filter_stack.unavailable").into();
                cx.notify();
            }
        }
    }

    pub(super) fn preview_stack_filter(
        &mut self,
        values: Option<&FilterValues>,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.stack_filter_session.clone() else {
            return;
        };
        let Some(doc) = self.doc.as_ref() else { return };
        let output = match values {
            Some(values) => {
                let mut stack = session.stack.clone();
                stack.effects[session.index].values =
                    values.0.iter().map(|(k, v)| ((*k).into(), *v)).collect();
                render_layer(
                    &self.registry,
                    &session.original,
                    &session.source,
                    &stack,
                    doc,
                )
            }
            None => Ok((
                session.original.as_raster().unwrap().tiles.clone(),
                session.original.smart.clone(),
            )),
        };
        match output {
            Ok((tiles, smart)) => {
                let doc = self.doc.as_mut().unwrap();
                if let Some(layer) = doc.tree.find_mut(session.layer) {
                    if let Some(raster) = layer.as_raster_mut() {
                        raster.tiles = tiles;
                    }
                    layer.smart = smart;
                    layer.styled = None;
                }
                doc.add_damage(doc.canvas_rect());
                self.after_change(cx);
            }
            Err(error) => {
                log::warn!("filter stack preview: {error:#}");
                self.status = t("filter_stack.render_failed").into();
                cx.notify();
            }
        }
    }

    pub(super) fn cancel_stack_filter(&mut self, cx: &mut Context<Self>) {
        self.preview_stack_filter(None, cx);
        self.stack_filter_session = None;
    }

    pub(super) fn commit_stack_filter(&mut self, values: &FilterValues, cx: &mut Context<Self>) {
        let Some(mut session) = self.stack_filter_session.clone() else {
            return;
        };
        session.stack.effects[session.index].values =
            values.0.iter().map(|(k, v)| ((*k).into(), *v)).collect();
        // Restore exact native before-images before capturing the history entry.
        self.preview_stack_filter(None, cx);
        let success = self.install_filter_stack(session.layer, &session.source, &session.stack, cx);
        if success {
            self.stack_filter_session = None;
        }
    }

    fn install_filter_stack(
        &mut self,
        id: LayerId,
        source: &schist_core::TileMap,
        stack: &FilterStack,
        cx: &mut Context<Self>,
    ) -> bool {
        let result = (|| -> anyhow::Result<_> {
            let doc = self
                .doc
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No document"))?;
            let layer = doc
                .tree
                .find(id)
                .ok_or_else(|| anyhow::anyhow!("Layer removed"))?;
            let (tiles, smart) = render_layer(&self.registry, layer, source, stack, doc)?;
            let extras = if stack.effects.is_empty() {
                filter_stack::without_stack(&layer.extras)
            } else {
                stack.blocks(layer, source)?
            };
            Ok((tiles, smart, extras))
        })();
        match result {
            Ok((tiles, smart, extras)) => {
                let doc = self.doc.as_mut().unwrap();
                let mut edit = doc.begin_edit(t("filter_stack.edit_history"));
                edit.replace_layer_tiles(id, tiles);
                if smart.is_some() {
                    edit.set_smart_object(id, smart);
                }
                edit.set_extras(id, extras);
                edit.commit();
                if let Some(layer) = doc.tree.find_mut(id) {
                    layer.styled = None;
                }
                self.status = t("filter_stack.updated").into();
                self.stack_filter_session = None;
                self.after_change(cx);
                true
            }
            Err(error) => {
                log::warn!("filter stack commit: {error:#}");
                self.status = t("filter_stack.render_failed").into();
                cx.notify();
                false
            }
        }
    }

    pub fn change_filter_stack(&mut self, change: StackChange, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_ref() else { return };
        let Some(layer) = doc.active_layer.and_then(|id| doc.tree.find(id)) else {
            return;
        };
        let id = layer.id;
        if layer.locked {
            self.status = t("filter_stack.unavailable").into();
            cx.notify();
            return;
        }
        if matches!(change, StackChange::Bake) {
            let extras = filter_stack::without_stack(&layer.extras);
            let doc = self.doc.as_mut().unwrap();
            let mut edit = doc.begin_edit(t("filter_stack.bake_history"));
            edit.set_extras(id, extras);
            edit.commit();
            self.after_change(cx);
            return;
        }
        let Ok((source, mut stack)) = source_and_stack(layer, doc.canvas_rect()) else {
            self.status = t("filter_stack.render_failed").into();
            cx.notify();
            return;
        };
        let len = stack.effects.len();
        match change {
            StackChange::Toggle(i) if i < len => {
                stack.effects[i].enabled = !stack.effects[i].enabled
            }
            StackChange::Up(i) if i > 0 && i < len => stack.effects.swap(i, i - 1),
            StackChange::Down(i) if i + 1 < len => stack.effects.swap(i, i + 1),
            StackChange::Remove(i) if i < len => {
                stack.effects.remove(i);
            }
            _ => return,
        }
        self.install_filter_stack(id, &source, &stack, cx);
    }
}

pub fn panel(ws: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    use gpui::prelude::*;
    let layer = ws
        .doc
        .as_ref()
        .and_then(|d| d.active_layer.and_then(|id| d.tree.find(id)));
    let supported = layer.is_some_and(|l| {
        l.as_raster().is_some()
            && !l.locked
            && l.shape.is_none()
            && !l.extras.iter().any(|b| b.key == *b"PsTx")
    });
    let stack = layer.and_then(|l| FilterStack::read(l).ok().flatten());
    let has_stack = layer.is_some_and(filter_stack::has_stack);
    let mut body = gpui::div()
        .flex()
        .flex_col()
        .gap_1()
        .border_t_1()
        .border_color(gpui::rgb(ui::palette().panel_edge))
        .pt_1();
    body = body.child(
        gpui::div()
            .flex()
            .flex_row()
            .gap_1()
            .items_center()
            .child(
                gpui::div()
                    .flex_1()
                    .text_size(gpui::px(11.0))
                    .child(t("filter_stack.title")),
            )
            .child(ui::button(
                t("filter_stack.add"),
                false,
                |ws, _w, cx| {
                    ws.filter_stack_picker = !ws.filter_stack_picker;
                    cx.notify();
                },
                cx,
            )),
    );
    if !supported {
        return body.child(
            gpui::div()
                .text_size(gpui::px(10.0))
                .child(t("filter_stack.unavailable")),
        );
    }
    if ws.filter_stack_picker {
        let mut filters: Vec<_> = ws
            .registry
            .filters()
            .filter(|f| schist_plugin_api::filter_stack::eligible(*f))
            .map(|f| (f.id(), f.name().to_string()))
            .collect();
        filters.sort_by(|a, b| a.1.cmp(&b.1));
        body = body
            .child(
                gpui::div()
                    .text_size(gpui::px(10.0))
                    .child(t("filter_stack.eligibility")),
            )
            .child(
                gpui::div()
                    .id("stack-filter-picker")
                    .flex()
                    .flex_col()
                    .max_h(gpui::px(180.0))
                    .overflow_y_scroll()
                    .children(filters.into_iter().map(|(id, name)| {
                        ui::button(
                            name,
                            false,
                            move |ws, _w, cx| ws.open_stack_filter(id, None, cx),
                            cx,
                        )
                    })),
            );
    }
    if let Some(stack) = stack {
        let mut rows = gpui::div()
            .id("stack-filter-list")
            .flex()
            .flex_col()
            .gap_1()
            .max_h(gpui::px(190.0))
            .overflow_y_scroll();
        for (index, effect) in stack.effects.iter().enumerate() {
            let filter = ws.registry.shared_filter(&effect.id);
            let id = filter.as_ref().map(|f| f.id());
            let name = filter
                .as_ref()
                .map(|f| f.name().to_string())
                .unwrap_or_else(|| tf!("filter_stack.missing", id = effect.id));
            rows = rows.child(
                gpui::div()
                    .id(("stack-effect", index))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(ui::button(
                        name,
                        false,
                        move |ws, _w, cx| {
                            if let Some(id) = id {
                                ws.open_stack_filter(id, Some(index), cx);
                            }
                        },
                        cx,
                    ))
                    .child(
                        gpui::div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .child(ui::button(
                                if effect.enabled {
                                    t("filter_stack.disable")
                                } else {
                                    t("filter_stack.enable")
                                },
                                false,
                                move |ws, _w, cx| {
                                    ws.change_filter_stack(StackChange::Toggle(index), cx)
                                },
                                cx,
                            ))
                            .child(ui::button(
                                t("filter_stack.up"),
                                false,
                                move |ws, _w, cx| {
                                    ws.change_filter_stack(StackChange::Up(index), cx)
                                },
                                cx,
                            ))
                            .child(ui::button(
                                t("filter_stack.down"),
                                false,
                                move |ws, _w, cx| {
                                    ws.change_filter_stack(StackChange::Down(index), cx)
                                },
                                cx,
                            ))
                            .child(ui::button(
                                t("filter_stack.remove"),
                                false,
                                move |ws, _w, cx| {
                                    ws.change_filter_stack(StackChange::Remove(index), cx)
                                },
                                cx,
                            )),
                    ),
            );
        }
        body = body.child(rows).child(
            gpui::div()
                .text_size(gpui::px(10.0))
                .child(t("filter_stack.note")),
        );
    }
    if has_stack {
        body = body.child(ui::button(
            t("filter_stack.bake"),
            false,
            |ws, _w, cx| ws.change_filter_stack(StackChange::Bake, cx),
            cx,
        ));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_plugin_api::{FilterPlugin, FilterValues, PluginRegistry};
    struct Red;
    impl FilterPlugin for Red {
        fn id(&self) -> &'static str {
            "red"
        }
        fn name(&self) -> &'static str {
            "red"
        }
        fn apply(&self, pixels: &mut [f32], _: usize, _: usize, _: &FilterValues) {
            for p in pixels.as_chunks_mut::<4>().0 {
                if p[3] > 0.0 {
                    p[0] = 1.0;
                }
            }
        }
    }
    fn setup() -> (Document, Layer, PluginRegistry) {
        let doc = Document::new("test", 8, 8, Depth::Eight);
        let mut layer = Layer::new_raster("source");
        layer
            .as_raster_mut()
            .unwrap()
            .tiles
            .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, Depth::Eight)
            .set(0, Rgba::BLACK);
        let mut registry = PluginRegistry::new();
        registry.register_filter(Box::new(Red));
        (doc, layer, registry)
    }
    #[test]
    fn filter_stack_smart_source_is_filtered_before_placement_and_removal_restores_source() {
        let (doc, mut layer, registry) = setup();
        let mut smart =
            schist_core::SmartObject::wrap(layer.as_raster().unwrap().tiles.clone(), "source");
        smart.transform.tx = 3.0;
        smart.transform.ty = 2.0;
        layer.as_raster_mut().unwrap().tiles = smart.render(doc.depth, doc.canvas_rect());
        layer.smart = Some(Box::new(smart));
        let (source, mut stack) = source_and_stack(&layer, doc.canvas_rect()).unwrap();
        stack.effects.push(FilterEffect {
            id: "red".into(),
            enabled: true,
            values: Default::default(),
            foreground: [0.0, 0.0, 0.0, 1.0],
            background: [1.0; 4],
        });
        let (rendered, smart) = render_layer(&registry, &layer, &source, &stack, &doc).unwrap();
        assert_eq!(rendered.pixel(3, 2).r, 1.0);
        assert_eq!(rendered.pixel(0, 0).a, 0.0);
        assert_eq!(source.pixel(0, 0).r, 0.0);
        assert_eq!(smart.unwrap().source.pixel(0, 0).r, 1.0);
        stack.effects.clear();
        let (restored, _) = render_layer(&registry, &layer, &source, &stack, &doc).unwrap();
        assert_eq!(restored.pixel(3, 2), Rgba::BLACK);
    }
    #[test]
    fn filter_stack_recovery_snapshot_excludes_preview_and_retains_recipe() {
        let (mut doc, mut layer, _) = setup();
        let (source, stack) = source_and_stack(&layer, doc.canvas_rect()).unwrap();
        layer.extras = stack.blocks(&layer, &source).unwrap();
        let original = layer.clone();
        let id = doc.push_layer(layer);
        doc.tree
            .find_mut(id)
            .unwrap()
            .as_raster_mut()
            .unwrap()
            .tiles
            .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, Depth::Eight)
            .set(0, Rgba::WHITE);
        let saved = committed_snapshot(&doc, &original);
        assert_eq!(
            saved
                .tree
                .find(id)
                .unwrap()
                .as_raster()
                .unwrap()
                .tiles
                .pixel(0, 0),
            Rgba::BLACK
        );
        assert_eq!(saved.tree.find(id).unwrap().extras, original.extras);
        assert_eq!(
            doc.tree
                .find(id)
                .unwrap()
                .as_raster()
                .unwrap()
                .tiles
                .pixel(0, 0),
            Rgba::WHITE
        );
        let reopened =
            schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&saved).unwrap()).unwrap();
        assert_eq!(
            FilterStack::read(&reopened.tree.layers[0]).unwrap(),
            Some(stack)
        );
        assert_eq!(
            reopened.tree.layers[0]
                .as_raster()
                .unwrap()
                .tiles
                .pixel(0, 0),
            Rgba::BLACK
        );
    }
}
