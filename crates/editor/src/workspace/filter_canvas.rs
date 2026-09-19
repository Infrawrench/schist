//! Filter handles share modal values and its transaction; they never edit tiles.
use super::*;
use schist_core::resample::Affine;
use schist_plugin_api::{filter_canvas::FilterCanvasControl, FilterValues};

#[derive(Default)]
pub(super) struct CanvasInteraction {
    selected: Option<(usize, usize)>,
    /// Offset preserves the grabbed point, avoiding a jump at pointer-down.
    drag: Option<(f32, f32)>,
}

struct CanvasSession {
    id: &'static str,
    values: FilterValues,
    controls: Vec<FilterCanvasControl>,
    region: IntRect,
    placement: Affine,
}
impl CanvasSession {
    fn size(&self) -> (f32, f32) {
        (self.region.width() as f32, self.region.height() as f32)
    }
    fn document_point(&self, p: (f32, f32)) -> (f32, f32) {
        self.placement
            .apply(p.0 + self.region.left as f32, p.1 + self.region.top as f32)
    }
    fn buffer_point(&self, p: (f32, f32)) -> Option<(f32, f32)> {
        let p = self.placement.invert()?.apply(p.0, p.1);
        Some((p.0 - self.region.left as f32, p.1 - self.region.top as f32))
    }
    fn selected_point(&self, selection: (usize, usize)) -> Option<(f32, f32)> {
        Some(
            self.controls
                .get(selection.0)?
                .geometry(&self.values, self.size())
                .handles
                .get(selection.1)?
                .point,
        )
    }
}

impl Workspace {
    fn filter_canvas_session(&self) -> Option<CanvasSession> {
        let Some(Modal::Filter { id, values, .. }) = &self.modal else {
            return None;
        };
        let controls = self
            .registry
            .filters()
            .find(|f| f.id() == *id)?
            .canvas_controls(values);
        if controls.is_empty() {
            return None;
        }
        let (region, placement) = if let Some(space) = self.stack_filter_canvas_space() {
            space
        } else {
            (self.filter_preview.as_ref()?.region, Affine::IDENTITY)
        };
        if region.is_empty() || placement.invert().is_none() {
            return None;
        }
        Some(CanvasSession {
            id,
            values: values.clone(),
            controls,
            region,
            placement,
        })
    }

    pub(crate) fn has_filter_canvas_controls(&self) -> bool {
        self.filter_canvas_session().is_some()
    }

    pub(super) fn paint_filter_canvas(
        &self,
        job: &mut PaintJob,
        screen: &impl Fn(f32, f32) -> Point<Pixels>,
    ) {
        let Some(session) = self.filter_canvas_session() else {
            return;
        };
        for (ci, control) in session.controls.iter().enumerate() {
            let geometry = control.geometry(&session.values, session.size());
            for guide in geometry.guides {
                let points: Vec<_> = guide
                    .into_iter()
                    .map(|p| {
                        let p = session.document_point(p);
                        screen(p.0, p.1)
                    })
                    .collect();
                job.polylines.push((points, gpui::rgb(0x44AAFF).into()));
            }
            for (hi, handle) in geometry.handles.into_iter().enumerate() {
                let p = session.document_point(handle.point);
                let p = screen(p.0, p.1);
                let r = px(5.0);
                job.markers.push(Marker {
                    bounds: Bounds {
                        origin: point(p.x - r, p.y - r),
                        size: size(r * 2.0, r * 2.0),
                    },
                    fill: gpui::rgb(0x44AAFF).into(),
                    selected: self.filter_canvas.selected == Some((ci, hi)),
                });
            }
        }
    }

    pub(crate) fn filter_canvas_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.canvas_bounds.contains(&ev.position) {
            return;
        }
        if !self.has_filter_canvas_controls() {
            return;
        }
        self.commit_focused_field();
        self.focused_field = None;
        let Some(session) = self.filter_canvas_session() else {
            return;
        };
        window.focus(&self.focus);
        window.claim_touch_drag();
        let p = self.doc_pos(self.to_local(ev.position));
        // View rotation preserves lengths, so this is a constant screen target.
        let target = 11.0 / self.zoom;
        let mut nearest = None;
        for (ci, control) in session.controls.iter().enumerate() {
            for (hi, handle) in control
                .geometry(&session.values, session.size())
                .handles
                .iter()
                .enumerate()
            {
                let h = session.document_point(handle.point);
                let distance = (h.0 - p.0).hypot(h.1 - p.1);
                if distance <= target && nearest.as_ref().is_none_or(|(_, _, d)| distance < *d) {
                    nearest = Some(((ci, hi), h, distance));
                }
            }
        }
        self.filter_canvas.drag = nearest.map(|(selection, h, _)| {
            self.filter_canvas.selected = Some(selection);
            (h.0 - p.0, h.1 - p.1)
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn filter_canvas_scroll(
        &mut self,
        ev: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.has_filter_canvas_controls() && self.canvas_bounds.contains(&ev.position) {
            self.on_scroll(ev, window, cx);
            cx.stop_propagation();
        }
    }

    fn move_filter_handle(&mut self, p: (f32, f32), cx: &mut Context<Self>) {
        let Some(mut session) = self.filter_canvas_session() else {
            self.filter_canvas = Default::default();
            return;
        };
        let Some((ci, hi)) = self.filter_canvas.selected else {
            return;
        };
        let Some(control) = session.controls.get(ci) else {
            return;
        };
        let Some(p) = session.buffer_point(p) else {
            return;
        };
        let specs = self
            .registry
            .filters()
            .find(|f| f.id() == session.id)
            .unwrap()
            .params();
        if !control.move_handle(hi, p, session.size(), &mut session.values, &specs) {
            return;
        }
        let mut preview = false;
        self.update_modal(|modal| {
            if let Modal::Filter {
                values,
                preview: enabled,
                ..
            } = modal
            {
                *values = session.values.clone();
                preview = *enabled;
            }
        });
        if preview {
            self.preview_filter(session.id, Some(&session.values), cx);
        }
        cx.notify();
    }

    /// Window listeners retain capture across the dialog card and window edges.
    pub(super) fn capture_filter_canvas(&self, window: &mut Window, cx: &mut Context<Self>) {
        // Register before a press so a quick move/release arriving before the
        // next repaint still belongs to the gesture (as with numeric sliders).
        if !self.has_filter_canvas_controls() {
            return;
        }
        let moves = cx.entity();
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _window, cx| {
            if phase != gpui::DispatchPhase::Capture {
                return;
            }
            moves.update(cx, |ws, cx| {
                let Some(offset) = ws.filter_canvas.drag else {
                    return;
                };
                if ev.pressed_button != Some(MouseButton::Left) {
                    ws.filter_canvas.drag = None;
                    cx.notify();
                    return;
                }
                let p = ws.doc_pos(ws.to_local(ev.position));
                ws.move_filter_handle((p.0 + offset.0, p.1 + offset.1), cx);
                cx.stop_propagation();
            });
        });
        let ups = cx.entity();
        window.on_mouse_event(move |ev: &MouseUpEvent, phase, _window, cx| {
            if phase != gpui::DispatchPhase::Capture || ev.button != MouseButton::Left {
                return;
            }
            ups.update(cx, |ws, cx| {
                if let Some(offset) = ws.filter_canvas.drag.take() {
                    let p = ws.doc_pos(ws.to_local(ev.position));
                    ws.move_filter_handle((p.0 + offset.0, p.1 + offset.1), cx);
                    cx.stop_propagation();
                    cx.notify();
                }
            });
        });
    }

    pub(super) fn filter_canvas_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.focused_field.is_some() {
            return false;
        }
        let Some(session) = self.filter_canvas_session() else {
            return false;
        };
        let Some(selection) = self.filter_canvas.selected else {
            return false;
        };
        let Some(p) = session.selected_point(selection) else {
            return false;
        };
        let step = if ev.keystroke.modifiers.shift {
            10.0
        } else {
            1.0
        };
        let delta = match ev.keystroke.key.as_str() {
            "left" => (-step, 0.0),
            "right" => (step, 0.0),
            "up" => (0.0, -step),
            "down" => (0.0, step),
            _ => return false,
        };
        self.move_filter_handle(session.document_point((p.0 + delta.0, p.1 + delta.1)), cx);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_canvas_maps_selection_and_placed_smart_object_coordinates() {
        let session = CanvasSession {
            id: "test",
            values: Default::default(),
            controls: vec![],
            region: IntRect::new(30, 40, 230, 140),
            placement: Affine::translate(90.0, -50.0)
                .then(&Affine::rotate(0.7))
                .then(&Affine::scale(2.0, 0.4)),
        };
        let local = (63.0, 27.0);
        let document = session.document_point(local);
        let restored = session.buffer_point(document).unwrap();
        assert!((restored.0 - local.0).abs() < 0.0001);
        assert!((restored.1 - local.1).abs() < 0.0001);
        let mut singular = session;
        singular.placement = Affine::scale(0.0, 1.0);
        assert!(singular.buffer_point(document).is_none());
    }
}
