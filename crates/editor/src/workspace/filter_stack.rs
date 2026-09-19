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

pub(super) fn source_and_stack(
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
) -> anyhow::Result<(
    schist_core::TileMap,
    Option<Box<schist_core::SmartObject>>,
    schist_core::TileMap,
)> {
    let filtered = schist_plugin_api::filter_stack::render(
        registry,
        stack,
        source,
        doc.depth,
        doc.icc_profile.clone(),
    )?;
    if let Some(mut smart) = layer.smart.clone() {
        smart.source = filtered.clone();
        smart.source_bounds = smart.source.content_bounds();
        let tiles = smart.render(doc.depth, doc.canvas_rect());
        Ok((tiles, Some(smart), filtered))
    } else {
        Ok((
            stack.place(&filtered, doc.depth, doc.canvas_rect()),
            None,
            filtered,
        ))
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
    pub(super) fn stack_filter_canvas_space(
        &self,
    ) -> Option<(IntRect, schist_core::resample::Affine)> {
        let session = self.stack_filter_session.as_ref()?;
        Some((
            session.stack.region,
            session
                .original
                .smart
                .as_ref()
                .map(|smart| smart.transform)
                .unwrap_or_else(|| {
                    session
                        .stack
                        .placement
                        .as_ref()
                        .map_or(schist_core::resample::Affine::IDENTITY, |placement| {
                            placement.matrix
                        })
                }),
        ))
    }

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

    /// Exclude both filter-dialog and transform-tool pixel previews from saves.
    pub(super) fn filter_stack_saved_document(&self, doc: &Document) -> Option<Document> {
        if self.doc.as_ref()?.id != doc.id {
            return None;
        }
        if let Some(session) = &self.stack_filter_session {
            return Some(committed_snapshot(doc, &session.original));
        }
        let (id, pixels) = self
            .registry
            .tools()
            .find(|tool| tool.id() == self.editor.active_tool)?
            .committed_layer_pixels()?;
        let mut original = doc.tree.find(id)?.clone();
        original.as_raster_mut()?.tiles = pixels.clone();
        original.styled = None;
        Some(committed_snapshot(doc, &original))
    }

    pub fn open_stack_filter(
        &mut self,
        id: &'static str,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        self.commit_pending_transform(cx);
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
                session.source.clone(),
            )),
        };
        match output {
            Ok((tiles, smart, _)) => {
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
            let effect = session.stack.effects[session.index].clone();
            let before = FilterStack::read(&session.original).ok().flatten();
            let previous = before
                .as_ref()
                .and_then(|stack| stack.effects.get(session.index));
            let change = if previous.is_none() {
                Some(recorded_actions::StackOperation::Add { effect })
            } else if previous != Some(&effect) {
                Some(recorded_actions::StackOperation::Set {
                    index: session.index,
                    effect,
                })
            } else {
                None
            };
            if let Some(change) = change {
                self.record_action_step(recorded_actions::Step::Stack { change });
            }
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
            let (tiles, smart, filtered) = render_layer(&self.registry, layer, source, stack, doc)?;
            let extras = if stack.effects.is_empty() {
                filter_stack::without_stack(&layer.extras)
            } else {
                stack.blocks_with_render(layer, source, &filtered)?
            };
            Ok((tiles, smart, extras))
        })();
        match result {
            Ok((tiles, smart, extras)) => {
                let doc = self.doc.as_mut().unwrap();
                let mut edit = doc.begin_edit(t("filter_stack.edit_history"));
                edit.replace_layer_render(id, tiles);
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
        self.commit_pending_transform(cx);
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
            self.record_action_step(recorded_actions::Step::Stack {
                change: recorded_actions::StackOperation::Bake,
            });
            self.after_change(cx);
            return;
        }
        let Ok((source, mut stack)) = source_and_stack(layer, doc.canvas_rect()) else {
            self.status = t("filter_stack.render_failed").into();
            cx.notify();
            return;
        };
        let len = stack.effects.len();
        let operation = match change {
            StackChange::Toggle(i) if i < len => recorded_actions::StackOperation::Enable {
                index: i,
                id: stack.effects[i].id.clone(),
                enabled: !stack.effects[i].enabled,
            },
            StackChange::Up(i) if i > 0 && i < len => recorded_actions::StackOperation::Move {
                index: i,
                to: i - 1,
                id: stack.effects[i].id.clone(),
            },
            StackChange::Down(i) if i + 1 < len => recorded_actions::StackOperation::Move {
                index: i,
                to: i + 1,
                id: stack.effects[i].id.clone(),
            },
            StackChange::Remove(i) if i < len => recorded_actions::StackOperation::Remove {
                index: i,
                id: stack.effects[i].id.clone(),
            },
            _ => return,
        };
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
        if self.install_filter_stack(id, &source, &stack, cx) {
            self.record_action_step(recorded_actions::Step::Stack { change: operation });
        }
    }
}

pub fn panel(ws: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    use gpui::prelude::*;
    use schist_ui::{Button, IconButton};
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
    let row_h = if ui::touch() { 32.0 } else { 24.0 };
    let mut filters: Vec<(gpui::SharedString, Option<&'static str>)> = ws
        .registry
        .filters()
        .filter(|f| schist_plugin_api::filter_stack::eligible(*f))
        .map(|f| (f.name().to_string().into(), Some(f.id())))
        .collect();
    filters.sort_by(|a, b| a.0.cmp(&b.0));
    let mut header = gpui::div().flex().items_center().gap_1().flex_none().child(
        Button::bare("stack-help")
            .ghost()
            .px_0()
            .flex_1()
            .min_w(gpui::px(0.0))
            .justify_start()
            .child(
                gpui::div()
                    .truncate()
                    .text_size(gpui::px(11.0))
                    .child(layer.map_or_else(|| t("common.none").to_string(), |l| l.name.clone())),
            )
            .tooltip(
                if supported {
                    t("filter_stack.note")
                } else {
                    t("filter_stack.unavailable")
                },
                None,
            ),
    );
    if has_stack {
        header = header.child(
            IconButton::new("stack-bake", "merge-down")
                .tooltip(t("filter_stack.bake"), None)
                .on_click(
                    cx.listener(|ws, _e, _w, cx| ws.change_filter_stack(StackChange::Bake, cx)),
                ),
        );
    }
    if supported {
        header = header.child(ui::searchable_dropdown_above(
            &ws.dropdown,
            &ws.dropdown_search,
            ui::Dropdown {
                popup: Popup::Field("stack-filter-picker"),
                is_open: ws.open_popup == Some(Popup::Field("stack-filter-picker")),
                current: None,
                label: t("filter_stack.add").into(),
                width: 86.0,
                options: filters,
            },
            |ws, id, cx| {
                if let Some(id) = id {
                    ws.open_stack_filter(id, None, cx);
                }
            },
            cx,
        ));
    }
    let mut body = gpui::div()
        .flex()
        .flex_col()
        .flex_grow()
        .min_h(gpui::px(0.0))
        .gap_1()
        .child(header);
    if !supported {
        body = body.child(
            gpui::div()
                .text_size(gpui::px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(t("filter_stack.unavailable")),
        );
    }
    if let Some(stack) = stack {
        let count = stack.effects.len();
        let mut rows = gpui::div()
            .id("stack-filter-list")
            .flex()
            .flex_col()
            .flex_grow()
            .min_h(gpui::px(0.0))
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
                    .items_center()
                    .flex_none()
                    .h(gpui::px(row_h))
                    .child(
                        IconButton::new(
                            ("stack-toggle", index),
                            if effect.enabled { "eye" } else { "eye-off" },
                        )
                        .size(row_h)
                        .icon_size(13.0)
                        .tooltip(
                            if effect.enabled {
                                t("filter_stack.disable")
                            } else {
                                t("filter_stack.enable")
                            },
                            None,
                        )
                        .on_click(cx.listener(move |ws, _e, _w, cx| {
                            ws.change_filter_stack(StackChange::Toggle(index), cx)
                        })),
                    )
                    .child(
                        Button::bare(("stack-edit", index))
                            .ghost()
                            .px_1()
                            .flex_1()
                            .min_w(gpui::px(0.0))
                            .justify_start()
                            .disabled(id.is_none())
                            .tooltip(name.clone(), None)
                            .child(gpui::div().truncate().text_size(gpui::px(11.0)).child(name))
                            .on_click(cx.listener(move |ws, _e, _w, cx| {
                                if let Some(id) = id {
                                    ws.open_stack_filter(id, Some(index), cx);
                                }
                            })),
                    )
                    .child(
                        Button::new(("stack-up", index), "↑")
                            .ghost()
                            .px_0()
                            .w(gpui::px(row_h))
                            .disabled(index == 0)
                            .tooltip(t("filter_stack.up"), None)
                            .on_click(cx.listener(move |ws, _e, _w, cx| {
                                ws.change_filter_stack(StackChange::Up(index), cx)
                            })),
                    )
                    .child(
                        Button::new(("stack-down", index), "↓")
                            .ghost()
                            .px_0()
                            .w(gpui::px(row_h))
                            .disabled(index + 1 == count)
                            .tooltip(t("filter_stack.down"), None)
                            .on_click(cx.listener(move |ws, _e, _w, cx| {
                                ws.change_filter_stack(StackChange::Down(index), cx)
                            })),
                    )
                    .child(
                        IconButton::new(("stack-remove", index), "trash")
                            .size(row_h)
                            .icon_size(13.0)
                            .tooltip(t("filter_stack.remove"), None)
                            .on_click(cx.listener(move |ws, _e, _w, cx| {
                                ws.change_filter_stack(StackChange::Remove(index), cx)
                            })),
                    ),
            );
        }
        body = body.child(rows);
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
        let (rendered, smart, _) = render_layer(&registry, &layer, &source, &stack, &doc).unwrap();
        assert_eq!(rendered.pixel(3, 2).r, 1.0);
        assert_eq!(rendered.pixel(0, 0).a, 0.0);
        assert_eq!(source.pixel(0, 0).r, 0.0);
        assert_eq!(smart.unwrap().source.pixel(0, 0).r, 1.0);
        stack.effects.clear();
        let (restored, _, _) = render_layer(&registry, &layer, &source, &stack, &doc).unwrap();
        assert_eq!(restored.pixel(3, 2), Rgba::BLACK);
    }
    #[test]
    fn filter_stack_edit_after_transform_refreshes_source_cache_and_keeps_placement() {
        use schist_core::{Affine, Filter};
        for smart in [false, true] {
            let (mut doc, mut layer, registry) = setup();
            if smart {
                layer.smart = Some(Box::new(schist_core::SmartObject::wrap(
                    layer.as_raster().unwrap().tiles.clone(),
                    "source",
                )));
            }
            let (source, mut stack) = source_and_stack(&layer, doc.canvas_rect()).unwrap();
            stack.effects.push(FilterEffect {
                id: "red".into(),
                enabled: true,
                values: Default::default(),
                foreground: [0.0; 4],
                background: [1.0; 4],
            });
            let (tiles, so, filtered) =
                render_layer(&registry, &layer, &source, &stack, &doc).unwrap();
            layer.as_raster_mut().unwrap().tiles = tiles;
            layer.smart = so;
            layer.extras = stack
                .blocks_with_render(&layer, &source, &filtered)
                .unwrap();
            let id = doc.push_layer(layer);
            let mut edit = doc.begin_edit("transform");
            edit.transform_layer(
                id,
                &Affine::translate(3.0, 2.0).then(&Affine::scale(2.0, 2.0)),
                Filter::Nearest,
                IntRect::from_size(8, 8),
            );
            edit.commit();
            let layer = doc.tree.find(id).unwrap();
            let (source, mut stack) = source_and_stack(layer, doc.canvas_rect()).unwrap();
            assert_eq!(source.pixel(0, 0), Rgba::BLACK);
            stack.effects[0].enabled = false;
            let (tiles, so, filtered) =
                render_layer(&registry, layer, &source, &stack, &doc).unwrap();
            assert_eq!(tiles.pixel(3, 2), Rgba::BLACK);
            assert_eq!(tiles.pixel(0, 0).a, 0.0);
            let extras = stack.blocks_with_render(layer, &source, &filtered).unwrap();
            let mut edit = doc.begin_edit("disable");
            edit.replace_layer_render(id, tiles);
            if so.is_some() {
                edit.set_smart_object(id, so);
            }
            edit.set_extras(id, extras);
            edit.commit();
            let mut edit = doc.begin_edit("transform again");
            edit.transform_layer(
                id,
                &Affine::translate(1.0, 1.0),
                Filter::Nearest,
                IntRect::from_size(8, 8),
            );
            edit.commit();
            assert_eq!(
                doc.tree
                    .find(id)
                    .unwrap()
                    .as_raster()
                    .unwrap()
                    .tiles
                    .pixel(4, 3),
                Rgba::BLACK
            );
            let layer = doc.tree.find(id).unwrap();
            let mut stack = FilterStack::read(layer).unwrap().unwrap();
            stack.effects.clear();
            let (restored, _, _) = render_layer(&registry, layer, &source, &stack, &doc).unwrap();
            assert_eq!(restored.pixel(4, 3), Rgba::BLACK);
        }
    }

    #[test]
    fn filter_stack_recovery_snapshot_excludes_preview_and_retains_recipe() {
        let (mut doc, mut layer, _) = setup();
        let (source, stack) = source_and_stack(&layer, doc.canvas_rect()).unwrap();
        layer.extras = stack.blocks(&layer, &source).unwrap();
        let id = doc.push_layer(layer);
        let mut edit = doc.begin_edit("transform");
        edit.transform_layer(
            id,
            &schist_core::Affine::scale(2.0, 2.0),
            schist_core::Filter::Nearest,
            IntRect::from_size(8, 8),
        );
        edit.commit();
        let original = doc.tree.find(id).unwrap().clone();
        let stack = FilterStack::read(&original).unwrap().unwrap();
        assert!(original
            .extras
            .iter()
            .any(|b| b.key == filter_stack::CACHE_KEY));
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
