//! Viewport image identity and presentation of asynchronous completions.

/// Tiles in an in-flight frame that have not been edited since submission.
/// Track only that frame's missing tiles, so memory stays bounded by the view.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default)]
pub(super) struct PendingTiles(rustc_hash::FxHashSet<schist_core::TileCoord>);

#[cfg(any(target_arch = "wasm32", test))]
impl PendingTiles {
    pub fn begin(&mut self, coords: &[schist_core::TileCoord]) {
        self.0.clear();
        self.0.extend(coords.iter().copied());
    }

    pub fn invalidate(&mut self, rect: &schist_core::IntRect) {
        // Iterating the bounded set also avoids walking an entire document
        // when a command damages everything while a small view is rendering.
        self.0
            .retain(|coord| coord.rect().intersect(rect).is_empty());
    }

    pub fn take(&mut self) -> rustc_hash::FxHashSet<schist_core::TileCoord> {
        std::mem::take(&mut self.0)
    }
}

/// Identifies the state a viewport image was assembled for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ViewportKey {
    pub revision: u64,
    /// Zoom, device-pixel pan, and scale factor as exact float bits. Rounding
    /// the pan here would shift a reused image when the fresh frame arrives.
    pub zoom: u32,
    pub offset: (u32, u32),
    pub scale_factor: u32,
    pub size: (u32, u32),
    pub color_epoch: u64,
    pub rotation: u32,
    /// The surround outside the document is baked into the image, so a
    /// theme change must invalidate it.
    pub surround: u32,
    pub seamless: bool,
}

/// Position a cached viewport texture in the requested view's device pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ViewportReprojection {
    pub scale: f32,
    pub translation: (f32, f32),
}

impl ViewportKey {
    /// Reuse the same transform during a gesture and while its replacement
    /// renders asynchronously after settling. Gesture lifetime must not decide
    /// where an image is drawn. Older content is useful while painting, too.
    pub fn reprojection_for(self, requested: Self) -> Option<ViewportReprojection> {
        if self.revision > requested.revision
            || (Self {
                revision: requested.revision,
                zoom: requested.zoom,
                offset: requested.offset,
                ..self
            }) != requested
        {
            return None;
        }
        let old_zoom = f32::from_bits(self.zoom);
        let zoom = f32::from_bits(requested.zoom);
        if !old_zoom.is_finite() || old_zoom <= 0.0 || !zoom.is_finite() || zoom <= 0.0 {
            return None;
        }
        let scale = zoom / old_zoom;
        let centre = (self.size.0 as f32 / 2.0, self.size.1 as f32 / 2.0);
        let old = (f32::from_bits(self.offset.0), f32::from_bits(self.offset.1));
        let new = (
            f32::from_bits(requested.offset.0),
            f32::from_bits(requested.offset.1),
        );
        // Uniform scaling commutes with the shared rotation about the centre:
        // s1 = k*s0 + (1-k)*c + R((k-1)*c - k*o0 + o1).
        let u = (
            (scale - 1.0) * centre.0 - scale * old.0 + new.0,
            (scale - 1.0) * centre.1 - scale * old.1 + new.1,
        );
        let (sin, cos) = f32::from_bits(self.rotation).sin_cos();
        Some(ViewportReprojection {
            scale,
            translation: (
                (1.0 - scale) * centre.0 + u.0 * cos - u.1 * sin,
                (1.0 - scale) * centre.1 + u.0 * sin + u.1 * cos,
            ),
        })
    }
}

#[cfg(any(target_arch = "wasm32", test))]
impl ViewportKey {
    /// The caller checks document identity and GPU lifetime separately.
    /// An intermediate revision is useful while edits arrive faster than
    /// GPU completions, but its view must still match and it must never
    /// replace an image of a newer revision (such as a CPU fallback).
    pub fn can_present_for(self, requested: Self, displayed: Option<Self>) -> bool {
        self.revision <= requested.revision
            && Self {
                revision: requested.revision,
                ..self
            } == requested
            && displayed.is_none_or(|shown| shown.revision <= self.revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_flight_tiles_survive_edits_elsewhere_but_never_restore_stale_pixels() {
        use schist_core::{IntRect, TileCoord};
        let coords: Vec<_> = (0..16).map(|tx| TileCoord { tx, ty: 0 }).collect();
        let mut pending = PendingTiles::default();
        pending.begin(&coords);
        pending.invalidate(&IntRect::new(1, 1, 2, 2));
        pending.invalidate(&IntRect::new(3, 3, 4, 4));
        let valid = pending.take();
        assert_eq!(valid.len(), 15);
        assert!(!valid.contains(&coords[0]));
        assert!(coords[1..].iter().all(|c| valid.contains(c)));
        assert!(pending.take().is_empty());

        pending.begin(&coords);
        pending.invalidate(&IntRect::new(0, 0, 30000, 30000));
        assert!(pending.take().is_empty());
        pending.begin(&coords[..1]);
        assert_eq!(pending.take().len(), 1);
    }

    fn frame(revision: u64) -> ViewportKey {
        ViewportKey {
            revision,
            zoom: 1.0f32.to_bits(),
            offset: (0, 0),
            scale_factor: 1.0f32.to_bits(),
            size: (800, 600),
            color_epoch: 0,
            rotation: 0.0f32.to_bits(),
            surround: 0x343434,
            seamless: false,
        }
    }

    #[test]
    fn continuous_drag_presents_frames_before_input_stops() {
        let mut displayed = frame(0);
        for revision in 1..10 {
            let completed = frame(revision);
            // Another pointer event landed while this frame was rendering.
            let requested = frame(revision + 1);
            assert!(completed.can_present_for(requested, Some(displayed)));
            displayed = completed;
            // An intermediate frame still misses the exact viewport cache
            // key, so the next paint will request the latest revision.
            assert_ne!(displayed, requested);
        }
        let released = frame(10);
        assert!(released.can_present_for(released, Some(displayed)));
    }

    #[test]
    fn completion_cannot_replace_a_newer_frame() {
        assert!(!frame(2).can_present_for(frame(4), Some(frame(3))));
        assert!(!frame(4).can_present_for(frame(3), None));
    }

    #[test]
    fn first_frame_can_present_with_more_edits_pending() {
        assert!(frame(1).can_present_for(frame(2), None));
        assert!(frame(2).can_present_for(frame(2), None));
    }

    /// Independent document-to-screen mapping used by a freshly rendered frame.
    fn screen_point(key: ViewportKey, document: (f32, f32)) -> (f32, f32) {
        let zoom = f32::from_bits(key.zoom) * f32::from_bits(key.scale_factor);
        let centre = (key.size.0 as f32 / 2.0, key.size.1 as f32 / 2.0);
        let x = document.0 * zoom + f32::from_bits(key.offset.0) - centre.0;
        let y = document.1 * zoom + f32::from_bits(key.offset.1) - centre.1;
        let (sin, cos) = f32::from_bits(key.rotation).sin_cos();
        (centre.0 + x * cos - y * sin, centre.1 + x * sin + y * cos)
    }

    #[test]
    fn cached_zoom_and_pan_match_fresh_frames_through_gpu_handoff() {
        for sf in [1.0f32, 1.25, 2.0] {
            for rotation in [0.0f32, 0.7, -1.5] {
                for zoom in [0.6f32, 1.0, 1.8] {
                    let cached = ViewportKey {
                        offset: ((17.375 * sf).to_bits(), (-6.125 * sf).to_bits()),
                        scale_factor: sf.to_bits(),
                        rotation: rotation.to_bits(),
                        ..frame(1)
                    };
                    // A fractional pan/zoom settles, then content is edited
                    // before the replacement arrives. Keep using this same
                    // projection for every pending frame, regardless of timers.
                    let requested = ViewportKey {
                        revision: 2,
                        zoom: zoom.to_bits(),
                        offset: ((-45.625 * sf).to_bits(), (32.25 * sf).to_bits()),
                        ..cached
                    };
                    let projection = cached.reprojection_for(requested).unwrap();
                    for point in [(0.0, 0.0), (173.25, 213.75), (799.0, 599.0)] {
                        let before = screen_point(cached, point);
                        let pending = (
                            before.0 * projection.scale + projection.translation.0,
                            before.1 * projection.scale + projection.translation.1,
                        );
                        let fresh = screen_point(requested, point);
                        assert!((pending.0 - fresh.0).abs() < 0.001);
                        assert!((pending.1 - fresh.1).abs() < 0.001);
                    }
                    // An earlier zoom completion must not replace this view.
                    assert!(!cached.can_present_for(requested, Some(cached)));
                    assert!(requested.can_present_for(requested, Some(cached)));
                    assert_eq!(
                        requested.reprojection_for(requested).unwrap(),
                        ViewportReprojection {
                            scale: 1.0,
                            translation: (0.0, 0.0),
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn reprojection_rejects_incompatible_cached_images() {
        let cached = frame(1);
        for requested in [
            ViewportKey {
                revision: 0,
                ..cached
            },
            ViewportKey {
                size: (1024, 768),
                ..cached
            },
            ViewportKey {
                scale_factor: 2.0f32.to_bits(),
                ..cached
            },
            ViewportKey {
                rotation: 0.5f32.to_bits(),
                ..cached
            },
            ViewportKey {
                color_epoch: 1,
                ..cached
            },
            ViewportKey {
                surround: 0x121212,
                ..cached
            },
            ViewportKey {
                seamless: true,
                ..cached
            },
            ViewportKey {
                zoom: 0.0f32.to_bits(),
                ..cached
            },
            ViewportKey {
                zoom: f32::NAN.to_bits(),
                ..cached
            },
        ] {
            assert!(
                cached.reprojection_for(requested).is_none(),
                "{requested:?}"
            );
        }
    }

    #[test]
    fn completion_must_match_the_requested_view() {
        let completed = frame(1);
        let requested = frame(2);
        for changed in [
            ViewportKey {
                seamless: true,
                ..requested
            },
            ViewportKey {
                zoom: 2.0f32.to_bits(),
                ..requested
            },
            ViewportKey {
                offset: (10.0f32.to_bits(), 0),
                ..requested
            },
            ViewportKey {
                scale_factor: 2.0f32.to_bits(),
                ..requested
            },
            ViewportKey {
                size: (1024, 768),
                ..requested
            },
            ViewportKey {
                color_epoch: 1,
                ..requested
            },
            ViewportKey {
                rotation: 0.5f32.to_bits(),
                ..requested
            },
            ViewportKey {
                surround: 0x121212,
                ..requested
            },
        ] {
            assert!(!completed.can_present_for(changed, None), "{changed:?}");
        }
    }
}
