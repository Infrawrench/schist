//! A layer style is a single resident graph, including its blend order.
use super::*;
use schist_core::style::GlowFalloff;
use schist_fx::{ComputeProgram, ComputeShader, ComputeSource};

static STYLE: ComputeShader = ComputeShader::new(
    "layer-style",
    concat!(
        include_str!("../../pixel-ops/src/blend.wgsl"),
        include_str!("gpu.wgsl")
    ),
);

struct Builder {
    program: ComputeProgram,
    rect: IntRect,
}

impl Builder {
    fn emit(
        &mut self,
        source: ComputeSource,
        auxiliary: ComputeSource,
        args: Vec<f32>,
        channels: usize,
    ) -> ComputeSource {
        let (w, h) = (self.rect.width() as usize, self.rect.height() as usize);
        let result = self.program.push(
            &STYLE,
            source,
            auxiliary,
            args,
            w * h * channels,
            [w as u32, h as u32, channels as u32],
        );
        self.program.steps.last_mut().unwrap().invocations = w * h;
        self.program.work = self.program.work.saturating_add(w * h * 32);
        result
    }

    fn blur(&mut self, source: ComputeSource, radius: f32) -> ComputeSource {
        if radius < 0.5 {
            return source;
        }
        let p = schist_fx::plane::alpha_blur_program(
            self.rect.width() as usize,
            self.rect.height() as usize,
            &blur::box_radii(radius / 3.0f32.sqrt()),
            false,
        );
        self.program.append(&p, source)
    }

    fn offset(&mut self, source: ComputeSource, dx: f32, dy: f32) -> ComputeSource {
        if dx == 0.0 && dy == 0.0 {
            return source;
        }
        let n = (self.rect.width() * self.rect.height()) as usize;
        self.program.work = self.program.work.saturating_add(n * 16);
        self.program.push(
            &schist_fx::plane::ALPHA_OFFSET,
            source,
            source,
            vec![dx, dy],
            n,
            [self.rect.width() as u32, self.rect.height() as u32, 1],
        )
    }

    fn distance(&mut self, source: ComputeSource, limit: f32) -> ComputeSource {
        let n = (self.rect.width() * self.rect.height()) as usize;
        self.program.work = self.program.work.saturating_add(
            n.saturating_mul((2 * limit.ceil().max(1.0) as usize + 1).saturating_pow(2)),
        );
        self.program.push(
            &schist_fx::plane::SIGNED_DISTANCE,
            source,
            source,
            vec![limit],
            n,
            [self.rect.width() as u32, self.rect.height() as u32, 1],
        )
    }

    fn invert(&mut self, source: ComputeSource) -> ComputeSource {
        self.emit(source, source, vec![4.0], 1)
    }

    fn color(
        &mut self,
        alpha: ComputeSource,
        color: Rgba,
        spread: f32,
        solid: bool,
    ) -> ComputeSource {
        self.emit(
            alpha,
            alpha,
            vec![
                0.0,
                color.r,
                color.g,
                color.b,
                color.a,
                spread,
                u8::from(solid) as f32,
            ],
            4,
        )
    }

    fn mask(
        &mut self,
        source: ComputeSource,
        alpha: ComputeSource,
        opacity: f32,
        mask: u8,
    ) -> ComputeSource {
        self.emit(source, alpha, vec![8.0, opacity, mask as f32], 4)
    }

    fn blend(
        &mut self,
        top: ComputeSource,
        bottom: ComputeSource,
        mode: BlendMode,
    ) -> ComputeSource {
        self.emit(
            top,
            bottom,
            vec![
                7.0,
                mode as u8 as f32,
                self.rect.left as f32,
                self.rect.top as f32,
            ],
            4,
        )
    }

    fn shadow(&mut self, alpha: ComputeSource, s: &ShadowStyle, inner: bool) -> ComputeSource {
        let (dx, dy) = super::polar(s.angle, s.distance);
        let mut a = self.offset(alpha, dx, dy);
        if inner {
            a = self.invert(a);
        }
        a = self.blur(a, s.size);
        self.color(a, s.color, s.spread, false)
    }

    fn glow(&mut self, alpha: ComputeSource, g: &GlowStyle, inner: bool) -> ComputeSource {
        let mut a = alpha;
        if inner {
            a = self.invert(a);
            if !g.from_edge {
                a = self.blur(a, g.size);
                a = self.invert(a);
            }
        }
        let (spread, blur) = glow::parameters(g);
        a = match g.technique {
            Technique::Softer => {
                if matches!(g.falloff, GlowFalloff::Photoshop { .. }) {
                    if spread > 0 {
                        a = self.emit(a, a, vec![11.0, spread as f32, 0.0], 1);
                        a = self.emit(a, a, vec![11.0, spread as f32, 1.0], 1);
                    }
                    self.blur(a, blur)
                } else {
                    self.blur(a, g.size)
                }
            }
            Technique::Precise => {
                let d = self.distance(a, g.size + 2.0);
                self.emit(d, d, vec![6.0, 0.0, g.size, 1.0], 1)
            }
        };
        match g.falloff {
            GlowFalloff::Gaussian => self.color(a, g.color, g.spread, false),
            GlowFalloff::Photoshop { range, noise } => {
                a = self.emit(
                    a,
                    a,
                    vec![
                        12.0,
                        range,
                        noise,
                        self.rect.left as f32,
                        self.rect.top as f32,
                    ],
                    1,
                );
                self.color(a, g.color, 0.0, false)
            }
        }
    }
}

pub(super) fn render(
    base: &Plane,
    alpha: &[f32],
    content: IntRect,
    style: &LayerStyle,
) -> Option<Vec<f32>> {
    let cost = alpha.len().saturating_mul(128);
    if !schist_fx::backend().compute_available(cost) {
        return None;
    }
    let input = ComputeSource::Input(0);
    let mut alpha_source = ComputeSource::Input(1);
    let mut b = Builder {
        rect: base.rect,
        program: ComputeProgram {
            buffers: vec![alpha.to_vec()],
            steps: vec![],
            result: input,
            work: 0,
        },
    };
    let mut pixels = input;
    if let Some(blur) = style.blur.on().filter(|blur| blur.radius >= 0.5) {
        for c in 0..4 {
            let plane = b.emit(input, input, vec![1.0, c as f32], 1);
            let low = b.blur(plane, blur.radius);
            pixels = b.emit(low, pixels, vec![2.0, c as f32], 4);
        }
        pixels = b.emit(
            pixels,
            input,
            vec![3.0, u8::from(blur.preserve_alpha) as f32],
            4,
        );
        if !blur.preserve_alpha {
            alpha_source = b.blur(alpha_source, blur.radius);
        }
    }
    let alpha = alpha_source;
    let mut out = b.color(alpha, Rgba::TRANSPARENT, 0.0, true);
    if let Some(s) = style.drop_shadow.on() {
        let shadow = b.shadow(alpha, s, false);
        let shadow = b.mask(shadow, alpha, s.opacity, if s.knockout { 2 } else { 0 });
        out = b.blend(out, shadow, BlendMode::Normal);
    }
    if let Some(g) = style.outer_glow.on() {
        let glow = b.glow(alpha, g, false);
        let glow = b.mask(glow, alpha, g.opacity, 2);
        out = b.blend(out, glow, BlendMode::Normal);
    }
    out = b.blend(pixels, out, BlendMode::Normal);
    if let Some(o) = style.gradient_overlay.on() {
        let cx = (content.left + content.right) as f32 / 2.0;
        let cy = (content.top + content.bottom) as f32 / 2.0;
        let half = (content.width().max(content.height()) as f32 / 2.0).max(1.0) * o.scale.max(0.1);
        let radians = o.angle.to_radians();
        let gradient = b.emit(
            alpha,
            alpha,
            vec![
                9.0,
                base.rect.left as f32 - cx,
                base.rect.top as f32 - cy,
                radians.cos(),
                -radians.sin(),
                half,
                u8::from(o.shape == GradientShape::Radial) as f32,
                u8::from(o.reverse) as f32,
                o.from.r,
                o.from.g,
                o.from.b,
                o.from.a,
                o.to.r,
                o.to.g,
                o.to.b,
                o.to.a,
            ],
            4,
        );
        let gradient = b.mask(gradient, alpha, o.opacity, 1);
        out = b.blend(gradient, out, o.blend);
    }
    if let Some(o) = style.color_overlay.on() {
        let color = b.color(alpha, o.color, 0.0, true);
        let color = b.mask(color, alpha, o.opacity, 1);
        out = b.blend(color, out, o.blend);
    }
    if let Some(s) = style.satin.on() {
        let (dx, dy) = super::polar(s.angle, s.distance);
        let a = b.offset(alpha, dx, dy);
        let c = b.offset(alpha, -dx, -dy);
        let a = b.blur(a, s.size);
        let c = b.blur(c, s.size);
        let diff = b.emit(a, c, vec![5.0, u8::from(s.invert) as f32], 1);
        let satin = b.color(diff, s.color, 0.0, false);
        let satin = b.mask(satin, alpha, s.opacity, 1);
        out = b.blend(satin, out, s.blend);
    }
    if let Some(g) = style.inner_glow.on() {
        let glow = b.glow(alpha, g, true);
        let glow = b.mask(glow, alpha, g.opacity, 1);
        out = b.blend(glow, out, g.blend);
    }
    if let Some(s) = style.inner_shadow.on() {
        let shadow = b.shadow(alpha, s, true);
        let shadow = b.mask(shadow, alpha, s.opacity, 1);
        out = b.blend(shadow, out, s.blend);
    }
    if let Some(bevel) = style.bevel.on() {
        let mut height = b.blur(alpha, bevel.size.max(0.5));
        if bevel.soften > 0.0 {
            height = b.blur(height, bevel.soften);
        }
        let rad = bevel.angle.to_radians();
        let alt = bevel.altitude.to_radians();
        let kind = match bevel.style {
            BevelStyle_::OuterBevel => 0.0,
            BevelStyle_::InnerBevel => 1.0,
            _ => 2.0,
        };
        let sign = if bevel.style == BevelStyle_::PillowEmboss {
            -1.0
        } else {
            1.0
        };
        for (side, color, opacity, mode) in [
            (
                0.0,
                bevel.highlight,
                bevel.highlight_opacity,
                bevel.highlight_blend,
            ),
            (1.0, bevel.shadow, bevel.shadow_opacity, bevel.shadow_blend),
        ] {
            let shade = b.emit(
                height,
                alpha,
                vec![
                    10.0,
                    bevel.depth,
                    sign,
                    rad.cos() * alt.cos(),
                    -rad.sin() * alt.cos(),
                    alt.sin().max(0.001),
                    kind,
                    side,
                ],
                1,
            );
            let shade = b.color(shade, color, 0.0, false);
            let shade = b.mask(shade, alpha, opacity, u8::from(kind != 0.0));
            out = b.blend(shade, out, mode);
        }
    }
    if let Some(s) = style.stroke.on() {
        let (outer, inner) = match s.position {
            StrokePosition::Outside => (s.size, 0.0),
            StrokePosition::Inside => (0.0, s.size),
            StrokePosition::Center => (s.size / 2.0, s.size / 2.0),
        };
        let distance = b.distance(alpha, outer.max(inner) + 2.0);
        let band = b.emit(distance, distance, vec![6.0, -inner, outer, 0.0], 1);
        let stroke = b.color(band, s.color, 0.0, false);
        let stroke = b.mask(stroke, alpha, s.opacity, 0);
        out = b.blend(stroke, out, s.blend);
    }
    b.program.result = out;
    schist_fx::try_compute(&base.px, &b.program)
}
