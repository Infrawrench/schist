// Shared GPU blend formulas for compositing and resident layer styles.

// BlendMode discriminants, in schist-core enum order.
const M_PASS_THROUGH: u32 = 0u;
const M_NORMAL: u32 = 1u;
const M_DISSOLVE: u32 = 2u;
const M_DARKEN: u32 = 3u;
const M_MULTIPLY: u32 = 4u;
const M_COLOR_BURN: u32 = 5u;
const M_LINEAR_BURN: u32 = 6u;
const M_DARKER_COLOR: u32 = 7u;
const M_LIGHTEN: u32 = 8u;
const M_SCREEN: u32 = 9u;
const M_COLOR_DODGE: u32 = 10u;
const M_LINEAR_DODGE: u32 = 11u;
const M_LIGHTER_COLOR: u32 = 12u;
const M_OVERLAY: u32 = 13u;
const M_SOFT_LIGHT: u32 = 14u;
const M_HARD_LIGHT: u32 = 15u;
const M_VIVID_LIGHT: u32 = 16u;
const M_LINEAR_LIGHT: u32 = 17u;
const M_PIN_LIGHT: u32 = 18u;
const M_HARD_MIX: u32 = 19u;
const M_DIFFERENCE: u32 = 20u;
const M_EXCLUSION: u32 = 21u;
const M_SUBTRACT: u32 = 22u;
const M_DIVIDE: u32 = 23u;
const M_HUE: u32 = 24u;
const M_SATURATION: u32 = 25u;
const M_COLOR: u32 = 26u;
const M_LUMINOSITY: u32 = 27u;

// ---- pixel-ops mirror ----

fn mulc(b: f32, s: f32) -> f32 {
    return b * s;
}

fn screenc(b: f32, s: f32) -> f32 {
    return b + s - b * s;
}

fn hard_light(b: f32, s: f32) -> f32 {
    if (s <= 0.5) {
        return mulc(b, 2.0 * s);
    }
    return screenc(b, 2.0 * s - 1.0);
}

fn color_dodge(b: f32, s: f32) -> f32 {
    if (b <= 0.0) {
        return 0.0;
    } else if (s >= 1.0) {
        return 1.0;
    }
    return min(b / (1.0 - s), 1.0);
}

fn color_burn(b: f32, s: f32) -> f32 {
    if (b >= 1.0) {
        return 1.0;
    } else if (s <= 0.0) {
        return 0.0;
    }
    return 1.0 - min((1.0 - b) / s, 1.0);
}

fn soft_light(b: f32, s: f32) -> f32 {
    if (s <= 0.5) {
        return b - (1.0 - 2.0 * s) * b * (1.0 - b);
    }
    var d: f32;
    if (b <= 0.25) {
        d = ((16.0 * b - 12.0) * b + 4.0) * b;
    } else {
        d = sqrt(b);
    }
    return b + (2.0 * s - 1.0) * (d - b);
}

fn separable(mode: u32, b: f32, s: f32) -> f32 {
    var v: f32;
    switch mode {
        case 3u: {
            v = min(b, s);
        }
        // Darken
        case 4u: {
            v = mulc(b, s);
        }
        // Multiply
        case 5u: {
            v = color_burn(b, s);
        }
        // ColorBurn
        case 6u: {
            v = b + s - 1.0;
        }
        // LinearBurn
        case 8u: {
            v = max(b, s);
        }
        // Lighten
        case 9u: {
            v = screenc(b, s);
        }
        // Screen
        case 10u: {
            v = color_dodge(b, s);
        }
        // ColorDodge
        case 11u: {
            v = b + s;
        }
        // LinearDodge
        case 13u: {
            v = hard_light(s, b);
        }
        // Overlay
        case 14u: {
            v = soft_light(b, s);
        }
        // SoftLight
        case 15u: {
            v = hard_light(b, s);
        }
        // HardLight
        case 16u: {
            // VividLight
            if (s <= 0.5) {
                v = color_burn(b, 2.0 * s);
            } else {
                v = color_dodge(b, 2.0 * s - 1.0);
            }
        }
        case 17u: {
            v = b + 2.0 * s - 1.0;
        }
        // LinearLight
        case 18u: {
            // PinLight
            if (s <= 0.5) {
                v = min(b, 2.0 * s);
            } else {
                v = max(b, 2.0 * s - 1.0);
            }
        }
        case 19u: {
            // HardMix
            if (b + s >= 1.0) {
                v = 1.0;
            } else {
                v = 0.0;
            }
        }
        case 20u: {
            v = abs(b - s);
        }
        // Difference
        case 21u: {
            v = b + s - 2.0 * b * s;
        }
        // Exclusion
        case 22u: {
            v = b - s;
        }
        // Subtract
        case 23u: {
            // Divide
            if (s <= 0.0) {
                v = 1.0;
            } else {
                v = b / s;
            }
        }
        default: {
            v = s;
        }
        // Normal/PassThrough/…
    }
    return clamp(v, 0.0, 1.0);
}

fn lum3(c: vec3<f32>) -> f32 {
    return 0.3 * c.x + 0.59 * c.y + 0.11 * c.z;
}

fn clip_color(c: vec3<f32>) -> vec3<f32> {
    let l = lum3(c);
    let n = min(c.x, min(c.y, c.z));
    let x = max(c.x, max(c.y, c.z));
    var out = c;
    if (n < 0.0) {
        out = vec3(l) + (out - vec3(l)) * (l / (l - n));
    }
    if (x > 1.0) {
        out = vec3(l) + (out - vec3(l)) * ((1.0 - l) / (x - l));
    }
    return out;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    let d = l - lum3(c);
    return clip_color(c + vec3(d));
}

fn sat3(c: vec3<f32>) -> f32 {
    return max(c.x, max(c.y, c.z)) - min(c.x, min(c.y, c.z));
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    // Stable 3-element sort of channel indices by value, matching the CPU
    // reference's sort_by.
    var cc = c;
    var i0 = 0u;
    var i1 = 1u;
    var i2 = 2u;
    if (cc[i0] > cc[i1]) {
        let t = i0;
        i0 = i1;
        i1 = t;
    }
    if (cc[i1] > cc[i2]) {
        let t = i1;
        i1 = i2;
        i2 = t;
    }
    if (cc[i0] > cc[i1]) {
        let t = i0;
        i0 = i1;
        i1 = t;
    }
    var out = vec3(0.0);
    if (cc[i2] > cc[i0]) {
        out[i1] = (cc[i1] - cc[i0]) * s / (cc[i2] - cc[i0]);
        out[i2] = s;
    }
    return out;
}

fn blend_color(mode: u32, cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    switch mode {
        case 24u: {
            return set_lum(set_sat(cs, sat3(cb)), lum3(cb));
        }
        // Hue
        case 25u: {
            return set_lum(set_sat(cb, sat3(cs)), lum3(cb));
        }
        // Saturation
        case 26u: {
            return set_lum(cs, lum3(cb));
        }
        // Color
        case 27u: {
            return set_lum(cb, lum3(cs));
        }
        // Luminosity
        case 7u: {
            // DarkerColor
            if (lum3(cs) < lum3(cb)) {
                return cs;
            }
            return cb;
        }
        case 12u: {
            // LighterColor
            if (lum3(cs) > lum3(cb)) {
                return cs;
            }
            return cb;
        }
        default: {
            return vec3(
                separable(mode, cb.x, cs.x),
                separable(mode, cb.y, cs.y),
                separable(mode, cb.z, cs.z),
            );
        }
    }
}

fn dissolve_hash(x: i32, y: i32) -> f32 {
    var h = (u32(x) * 0x9E3779B9u) ^ (u32(y) * 0x85EBCA6Bu);
    h ^= h >> 16u;
    h *= 0x7FEB352Du;
    h ^= h >> 15u;
    h *= 0x846CA68Bu;
    h ^= h >> 16u;
    return f32(h >> 8u) / 16777216.0;
}

fn over(top: vec4<f32>, bottom: vec4<f32>) -> vec4<f32> {
    let a = top.a + bottom.a * (1.0 - top.a);
    if (a <= 1.1920929e-7) {
        return vec4(0.0);
    }
    let c = (top.rgb * top.a + bottom.rgb * bottom.a * (1.0 - top.a)) / a;
    return vec4(c, a);
}

fn blend_px(mode: u32, top: vec4<f32>, bottom: vec4<f32>, x: i32, y: i32) -> vec4<f32> {
    if (mode == M_DISSOLVE) {
        // Dissolve: source shown opaque with probability = source alpha.
        if (top.a > dissolve_hash(x, y)) {
            return over(vec4(top.rgb, 1.0), bottom);
        }
        return bottom;
    }
    let a_s = top.a;
    let a_b = bottom.a;
    if (a_s <= 0.0) {
        return bottom;
    }
    let bl = blend_color(mode, bottom.rgb, top.rgb);
    let a_o = a_s + a_b * (1.0 - a_s);
    if (a_o <= 0.0) {
        return vec4(0.0);
    }
    let co = (top.rgb * a_s * (1.0 - a_b) + bl * a_s * a_b + bottom.rgb * a_b * (1.0 - a_s)) / a_o;
    return vec4(co, a_o);
}
