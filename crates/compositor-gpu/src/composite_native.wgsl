// Native interpreter; prepended with composite_common.wgsl. Colour and
// alpha are independent: CMYK's fourth component is K, never opacity.
struct NativePixel {
    color: vec4<f32>,
    alpha: f32,
}

fn native_blank() -> NativePixel {
    return NativePixel(vec4(0.0), 0.0);
}

// Mirrors schist_color::convert's fallback D50 conversions. These are
// only used at explicit RGB blend/adjustment boundaries; display ICC
// conversion happens after native readback in schist_colormgmt.
fn to_linear(v: f32) -> f32 {
    if (v <= 0.04045) {
        return v / 12.92;
    }
    return pow((v + 0.055) / 1.055, 2.4);
}

fn from_linear(v: f32) -> f32 {
    if (v <= 0.0031308) {
        return v * 12.92;
    }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}

fn lab_f(t: f32) -> f32 {
    let d = 6.0 / 29.0;
    if (t > d * d * d) {
        return pow(t, 1.0 / 3.0);
    }
    return t / (3.0 * d * d) + 4.0 / 29.0;
}

fn lab_f_inv(t: f32) -> f32 {
    let d = 6.0 / 29.0;
    if (t > d) {
        return t * t * t;
    }
    return 3.0 * d * d * (t - 4.0 / 29.0);
}

fn native_to_rgb(p: NativePixel) -> vec4<f32> {
    if (globals.color_mode == 1u) {
        let c = clamp(p.color, vec4(0.0), vec4(1.0));
        return vec4(clamp((vec3(1.0) - c.xyz) * (1.0 - c.w), vec3(0.0), vec3(1.0)), p.alpha);
    }
    let lab = p.color.xyz * vec3(100.0, 255.0, 255.0) - vec3(0.0, 128.0, 128.0);
    let fy = (lab.x + 16.0) / 116.0;
    let x = lab_f_inv(fy + lab.y / 500.0) * 0.96422;
    let y = lab_f_inv(fy);
    let z = lab_f_inv(fy - lab.z / 200.0) * 0.82521;
    let rgb = vec3(
        from_linear(3.133856 * x - 1.616867 * y - 0.490615 * z),
        from_linear(-0.978769 * x + 1.916142 * y + 0.033454 * z),
        from_linear(0.071945 * x - 0.228991 * y + 1.405243 * z),
    );
    return vec4(clamp(rgb, vec3(0.0), vec3(1.0)), p.alpha);
}

fn native_from_rgb(p: vec4<f32>) -> NativePixel {
    if (globals.color_mode == 1u) {
        let k = 1.0 - max(max(p.r, p.g), p.b);
        if (k >= 1.0 - 1e-6) {
            return NativePixel(vec4(0.0, 0.0, 0.0, 1.0), p.a);
        }
        return NativePixel(vec4((vec3(1.0) - p.rgb - vec3(k)) / (1.0 - k), k), p.a);
    }
    let r = to_linear(p.r);
    let g = to_linear(p.g);
    let b = to_linear(p.b);
    let x = (0.4360747 * r + 0.3850649 * g + 0.1430804 * b) / 0.96422;
    let y = 0.2225045 * r + 0.7168786 * g + 0.0606169 * b;
    let z = (0.0139322 * r + 0.0971045 * g + 0.7141733 * b) / 0.82521;
    let fx = lab_f(x);
    let fy = lab_f(y);
    let fz = lab_f(z);
    return NativePixel(vec4(
        (116.0 * fy - 16.0) / 100.0,
        (500.0 * (fx - fy) + 128.0) / 255.0,
        (200.0 * (fy - fz) + 128.0) / 255.0,
        0.0,
    ), p.a);
}

fn native_src(row: i32, tile: u32, pixel: u32) -> NativePixel {
    if (row < 0) {
        return native_blank();
    }
    let slot = (u32(row) * globals.n_tiles + tile) * 6u;
    let xy = vec2(pixel % 256u, pixel / 256u) + vec2(u32(slots[slot + 4u]), u32(slots[slot + 5u]));
    let off = slots[slot + (xy.y / 256u) * 2u + xy.x / 256u];
    let px = (xy.y % 256u) * 256u + xy.x % 256u;
    if (off < 0) {
        return native_blank();
    }
    // Upload is five f32 samples at every depth; widening preserves the
    // authoritative native values, including out-of-gamut float samples.
    let b = u32(off) + px * 5u;
    return NativePixel(vec4(
        bitcast<f32>(src_words[b]), bitcast<f32>(src_words[b + 1u]),
        bitcast<f32>(src_words[b + 2u]), bitcast<f32>(src_words[b + 3u]),
    ), bitcast<f32>(src_words[b + 4u]));
}

fn native_blend(mode: u32, top: NativePixel, bottom: NativePixel, x: i32, y: i32) -> NativePixel {
    if (top.alpha <= 0.0) {
        return bottom;
    }
    if (mode == M_NORMAL || mode == M_PASS_THROUGH) {
        let a = top.alpha + bottom.alpha * (1.0 - top.alpha);
        if (a <= 1.1920929e-7) {
            return native_blank();
        }
        return NativePixel((top.color * top.alpha + bottom.color * bottom.alpha * (1.0 - top.alpha)) / a, a);
    }
    if (globals.color_mode == 2u || mode == M_DARKER_COLOR || mode == M_LIGHTER_COLOR || mode >= M_HUE) {
        return native_from_rgb(blend_px(mode, native_to_rgb(top), native_to_rgb(bottom), x, y));
    }
    // Separable CMYK modes operate on ink complements, including K.
    var out = top;
    for (var c = 0u; c < 4u; c++) {
        let t = 1.0 - top.color[c];
        let b = 1.0 - bottom.color[c];
        let p = blend_px(mode, vec4(vec3(t), top.alpha), vec4(vec3(b), bottom.alpha), x, y);
        out.color[c] = 1.0 - p.r;
        out.alpha = p.a;
    }
    return out;
}

@compute @workgroup_size(16, 16, 1)
fn composite_native(@builtin(global_invocation_id) gid: vec3<u32>) {
    let tile = gid.z;
    if (tile >= globals.n_tiles) {
        return;
    }
    let px = gid.y * TILE + gid.x;
    let orig = tile_origin[tile];
    let x = orig.x + i32(gid.x);
    let y = orig.y + i32(gid.y);
    var stack: array<NativePixel, MAX_DEPTH>;
    var snap: array<f32, MAX_DEPTH>;
    var sp = 1u;
    stack[0] = native_blank();
    for (var i = 0u; i < globals.n_ops; i++) {
        let op = ops[i];
        switch op.kind {
            case 0u: {
                var p = native_src(op.src_ref, tile, px);
                p.alpha *= mask_value(i, tile, x, y, px);
                stack[sp] = p;
                sp += 1u;
            }
            case 1u: {
                stack[sp] = native_blank();
                sp += 1u;
            }
            case 2u: {
                sp -= 1u;
                var p = stack[sp];
                p.alpha *= mask_value(i, tile, x, y, px);
                p.alpha *= op.opacity;
                stack[sp - 1u] = native_blend(op.mode, p, stack[sp - 1u], x, y);
            }
            case 3u: {
                sp -= 1u;
                let ba = snap[sp - 1u];
                if (ba > 0.0) {
                    var p = stack[sp];
                    // CPU first confines the source, then scales opacity.
                    p.alpha *= ba;
                    p.alpha *= op.opacity;
                    stack[sp - 1u] = native_blend(op.mode, p, stack[sp - 1u], x, y);
                }
            }
            case 4u: {
                snap[sp - 1u] = stack[sp - 1u].alpha;
            }
            case 5u: {
                let rgb = native_to_rgb(stack[sp - 1u]);
                let adjusted = adjust_px(i, tile, x, y, px, snap[sp - 1u], rgb);
                // Preserve the original separation for unchanged RGB,
                // including identity and alpha-only adjustments.
                // Match the CPU boundary's roundoff tolerance: an identity
                // LUT must not re-separate out-of-gamut native samples.
                if (any(abs(adjusted.rgb - rgb.rgb) > vec3(1e-6))) {
                    stack[sp - 1u] = native_from_rgb(adjusted);
                }
                stack[sp - 1u].alpha = adjusted.a;
            }
            case 6u: {
                stack[sp - 1u].alpha *= mask_value(i, tile, x, y, px);
            }
            default: {
            }
        }
    }
    let out = stack[0];
    let o = (tile * TILE_PIXELS + px) * 5u;
    out_f32[o] = out.color.x;
    out_f32[o + 1u] = out.color.y;
    out_f32[o + 2u] = out.color.z;
    out_f32[o + 3u] = out.color.w;
    out_f32[o + 4u] = out.alpha;
}
