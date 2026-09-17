//! Face geometry shared by desktop and library callers.
/// A face's box, as fractions of the photo's width and height.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FaceRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Two boxes overlapping by more than this (intersection over union)
/// are the same face: a tag drawn over a detection claims it.
pub const SAME_FACE_IOU: f32 = 0.3;

impl FaceRect {
    /// From pixel coordinates in a `width`x`height` image.
    pub fn from_pixels(x: f32, y: f32, w: f32, h: f32, width: f32, height: f32) -> FaceRect {
        FaceRect {
            x: x / width,
            y: y / height,
            w: w / width,
            h: h / height,
        }
        .clamped()
    }

    /// Kept inside the photo, with a positive size.
    pub fn clamped(self) -> FaceRect {
        let x0 = self.x.clamp(0.0, 1.0);
        let y0 = self.y.clamp(0.0, 1.0);
        let x1 = (self.x + self.w).clamp(0.0, 1.0);
        let y1 = (self.y + self.h).clamp(0.0, 1.0);
        FaceRect {
            x: x0.min(x1),
            y: y0.min(y1),
            w: (x1 - x0).abs(),
            h: (y1 - y0).abs(),
        }
    }

    /// Whether a point (in the same fractions) falls inside.
    pub fn contains(&self, fx: f32, fy: f32) -> bool {
        fx >= self.x && fy >= self.y && fx <= self.x + self.w && fy <= self.y + self.h
    }

    /// Intersection over union with another box, 0 when apart.
    pub fn overlap(&self, other: &FaceRect) -> f32 {
        let x = (self.x + self.w).min(other.x + other.w) - self.x.max(other.x);
        let y = (self.y + self.h).min(other.y + other.h) - self.y.max(other.y);
        if x <= 0.0 || y <= 0.0 {
            return 0.0;
        }
        let inter = x * y;
        let union = self.w * self.h + other.w * other.h - inter;
        if union <= 0.0 {
            0.0
        } else {
            inter / union
        }
    }

    /// Whether this and `other` are one face, by overlap.
    pub fn same_face(&self, other: &FaceRect) -> bool {
        self.overlap(other) > SAME_FACE_IOU
    }

    /// The square the recogniser and the avatars crop: centred on the
    /// box, its longer side times `grow`, in a `width`x`height` image,
    /// as pixel `(x, y, side)` kept inside the image.
    pub fn crop_square(&self, grow: f32, width: u32, height: u32) -> (u32, u32, u32) {
        let (w, h) = (width as f32, height as f32);
        let side = (self.w * w).max(self.h * h) * grow;
        let side = side.round().max(1.0).min(w).min(h);
        let cx = (self.x + self.w / 2.0) * w;
        let cy = (self.y + self.h / 2.0) * h;
        let x0 = (cx - side / 2.0).round().clamp(0.0, w - side);
        let y0 = (cy - side / 2.0).round().clamp(0.0, h - side);
        (x0 as u32, y0 as u32, side.round().max(1.0) as u32)
    }

    /// A stable text key, for caches keyed by face: quantised to a
    /// thousandth, so the same box read back from JSON matches.
    pub fn key(&self) -> String {
        format!("{:.3},{:.3},{:.3},{:.3}", self.x, self.y, self.w, self.h)
    }
}
