// Tile compositing as a per-pixel stack machine.
//
// The plan builder walks the layer tree once on the CPU and flattens it to
// an op program; every pixel of every tile in the batch then executes that
// program in a single dispatch, keeping its own small stack of colour values.
// Compositing has no cross-pixel dependencies, so this maps exactly.
//
// The math here mirrors schist-pixel-ops line for line — same formulas,
// same operand order, same guards — because the CPU compositor is the
// semantic contract and the parity tests hold this shader to it.

const TILE: u32 = 256u;
const TILE_PIXELS: u32 = 65536u;
const MAX_DEPTH: u32 = 12u;

// Op kinds.
const OP_PUSH_LAYER: u32 = 0u;
const OP_PUSH_BLANK: u32 = 1u;
const OP_BLEND: u32 = 2u;
const OP_CLIP_BLEND: u32 = 3u;
const OP_SNAPSHOT_ALPHA: u32 = 4u;
const OP_ADJUST: u32 = 5u;
const OP_MASK_TOP: u32 = 6u;

// Adjust flags.
const F_CONFINE: u32 = 1u;
const F_FILL: u32 = 2u;

// Source formats (matches TileBuf variants).
const FMT_U8: u32 = 0u;
const FMT_U16: u32 = 1u;
const FMT_F32: u32 = 2u;

struct Op {
    kind: u32,
    mode: u32,
    opacity: f32,
    flags: u32,
    src_ref: i32,
    src_fmt: u32,
    mask_ref: i32,
    lut: i32,
    mask_bounds: vec4<i32>, // left, top, right, bottom
    fill: vec4<f32>,
    mask_default: f32,
    direct: u32,
    dparams: i32,
    _p0: u32,
}

struct Globals {
    n_ops: u32,
    n_tiles: u32,
    color_mode: u32, // 0 RGB, 1 CMYK, 2 Lab
    _p1: u32,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<storage, read> ops: array<Op>;
@group(0) @binding(2) var<storage, read> tile_origin: array<vec2<i32>>;
@group(0) @binding(3) var<storage, read> src_words: array<u32>;
@group(0) @binding(4) var<storage, read> mask_words: array<u32>;
@group(0) @binding(5) var<storage, read> slots: array<i32>;
@group(0) @binding(6) var<storage, read> luts: array<f32>;
@group(0) @binding(7) var<storage, read_write> out_f32: array<f32>;
@group(0) @binding(8) var<storage, read> dparams: array<f32>;

// ---- sources ----

fn src_texel(row: i32, fmt: u32, tile: u32, pixel: u32) -> vec4<f32> {
    if (row < 0) {
        return vec4(0.0);
    }
    let slot = (u32(row) * globals.n_tiles + tile) * 6u;
    let xy = vec2(pixel % 256u, pixel / 256u) + vec2(u32(slots[slot + 4u]), u32(slots[slot + 5u]));
    let off = slots[slot + (xy.y / 256u) * 2u + xy.x / 256u];
    let px = (xy.y % 256u) * 256u + xy.x % 256u;
    if (off < 0) {
        return vec4(0.0);
    }
    let base = u32(off);
    switch fmt {
        case 0u: {
            let w = src_words[base + px];
            return vec4(
                f32(w & 0xFFu),
                f32((w >> 8u) & 0xFFu),
                f32((w >> 16u) & 0xFFu),
                f32((w >> 24u) & 0xFFu),
            ) / 255.0;
        }
        case 1u: {
            let w0 = src_words[base + px * 2u];
            let w1 = src_words[base + px * 2u + 1u];
            return vec4(
                f32(w0 & 0xFFFFu),
                f32(w0 >> 16u),
                f32(w1 & 0xFFFFu),
                f32(w1 >> 16u),
            ) / 65535.0;
        }
        default: {
            let b = base + px * 4u;
            return vec4(
                bitcast<f32>(src_words[b]),
                bitcast<f32>(src_words[b + 1u]),
                bitcast<f32>(src_words[b + 2u]),
                bitcast<f32>(src_words[b + 3u]),
            );
        }
    }
}

// LayerMask::value: default outside bounds, stored tiles (0 when sparse)
// inside.
fn mask_value(op_i: u32, tile: u32, x: i32, y: i32, px: u32) -> f32 {
    let op = ops[op_i];
    if (op.mask_ref < 0) {
        return 1.0;
    }
    let b = op.mask_bounds;
    if (x < b.x || y < b.y || x >= b.z || y >= b.w) {
        return op.mask_default;
    }
    let off = slots[(u32(op.mask_ref) * globals.n_tiles + tile) * 6u];
    if (off < 0) {
        return 0.0;
    }
    let w = mask_words[u32(off) + px / 4u];
    return f32((w >> ((px % 4u) * 8u)) & 0xFFu) / 255.0;
}

fn sample_lut(base: u32, v: f32) -> f32 {
    let x = clamp(v, 0.0, 1.0) * 255.0;
    let i = u32(x);
    if (i >= 255u) {
        return luts[base + 255u];
    }
    // Linear interpolation keeps 16/32-bit inputs from banding.
    let f = x - f32(i);
    let a = luts[base + i];
    return a + (luts[base + i + 1u] - a) * f;
}

fn apply_lut(lut: i32, c: vec3<f32>) -> vec3<f32> {
    let base = u32(lut) * 768u;
    return vec3(
        sample_lut(base, c.x),
        sample_lut(base + 256u, c.y),
        sample_lut(base + 512u, c.z),
    );
}

// Shared RGB adjustment boundary, also used by the native interpreter.
fn adjust_px(i: u32, tile: u32, x: i32, y: i32, px: u32, confine: f32, d: vec4<f32>) -> vec4<f32> {
    let op = ops[i];
    var result = d;
    var weight = op.opacity * mask_value(i, tile, x, y, px);
    if ((op.flags & F_CONFINE) != 0u) {
        weight *= confine;
    }
    if (weight > 0.0) {
        if ((op.flags & F_FILL) != 0u) {
            // Fill layers paint their colour rather than
            // transforming the backdrop.
            result = blend_px(op.mode, vec4(op.fill.rgb, weight), d, x, y);
        } else if (d.a > 0.0) {
            var adjusted: vec3<f32>;
            if (op.direct == D_NONE) {
                adjusted = apply_lut(op.lut, d.rgb);
            } else {
                adjusted = apply_direct(
                    op.direct,
                    u32(op.dparams),
                    d.rgb,
                );
            }
            // Mirrors the CPU compositor: the adjustment's
            // own blend mode applies, with the adjusted colour
            // as the source and `weight` as its alpha. It used
            // to be uploaded and then ignored, so every
            // adjustment rendered as Normal.
            if (op.mode == M_NORMAL) {
                result = vec4(d.rgb + (adjusted - d.rgb) * weight, d.a);
            } else {
                let blended = blend_px(op.mode, vec4(adjusted, weight), d, x, y);
                result = vec4(blended.rgb, d.a);
            }
        }
    }
    return result;
}

fn adj_arg(index: u32) -> f32 {
    return dparams[index];
}
