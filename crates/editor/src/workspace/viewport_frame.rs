//! Viewport image identity and presentation of asynchronous completions.

/// Identifies the state a viewport image was assembled for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ViewportKey {
    pub revision: u64,
    /// Zoom as raw bits and pan in device pixels.
    pub zoom: u32,
    pub offset: (i32, i32),
    pub size: (u32, u32),
    pub color_epoch: u64,
    pub rotation: u32,
    /// The surround outside the document is baked into the image, so a
    /// theme change must invalidate it.
    pub surround: u32,
    pub seamless: bool,
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

    fn frame(revision: u64) -> ViewportKey {
        ViewportKey {
            revision,
            zoom: 1.0f32.to_bits(),
            offset: (0, 0),
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
                offset: (10, 0),
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
