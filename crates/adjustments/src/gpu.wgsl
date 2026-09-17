const D_COLOR_BALANCE: u32 = 5u;
const D_VIBRANCE: u32 = 6u;
const D_PHOTO_FILTER: u32 = 7u;
const D_GRADIENT_MAP: u32 = 8u;
const D_SELECTIVE_COLOR: u32 = 9u;
const D_CHANNEL_MIXER: u32 = 10u;
const D_WHITE_BALANCE: u32 = 11u;
// Full-colour adjustment kinds (plan::D_*).
const D_NONE: u32 = 0u;
const D_HUE_SATURATION: u32 = 1u;
const D_BLACK_WHITE: u32 = 2u;
const D_THRESHOLD: u32 = 3u;
const D_POSTERIZE: u32 = 4u;

// ---- full-colour adjustments ----
//
// Shared with destructive adjustments. The host supplies adj_arg(index).
// Variable-length records retain the CPU reference operand order.

fn rem_euclid_f(a: f32, b: f32) -> f32 {
    let r = a % b;
    if (r < 0.0) {
        return r + b;
    }
    return r;
}

fn rgb_to_hsl(c: vec3<f32>) -> vec3<f32> {
    let mx = max(c.r, max(c.g, c.b));
    let mn = min(c.r, min(c.g, c.b));
    let l = (mx + mn) / 2.0;
    if (abs(mx - mn) < 1e-6) {
        return vec3(0.0, 0.0, l);
    }
    let d = mx - mn;
    var s: f32;
    if (l > 0.5) {
        s = d / (2.0 - mx - mn);
    } else {
        s = d / (mx + mn);
    }
    var h: f32;
    if (mx == c.r) {
        h = 60.0 * (((c.g - c.b) / d) % 6.0);
    } else if (mx == c.g) {
        h = 60.0 * ((c.b - c.r) / d + 2.0);
    } else {
        h = 60.0 * ((c.r - c.g) / d + 4.0);
    }
    return vec3(rem_euclid_f(h, 360.0), s, l);
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> vec3<f32> {
    if (s <= 1e-6) {
        return vec3(l, l, l);
    }
    let c = (1.0 - abs(2.0 * l - 1.0)) * s;
    let hp = rem_euclid_f(h, 360.0) / 60.0;
    let x = c * (1.0 - abs(hp % 2.0 - 1.0));
    var rgb: vec3<f32>;
    switch u32(hp) {
        case 0u: { rgb = vec3(c, x, 0.0); }
        case 1u: { rgb = vec3(x, c, 0.0); }
        case 2u: { rgb = vec3(0.0, c, x); }
        case 3u: { rgb = vec3(0.0, x, c); }
        case 4u: { rgb = vec3(x, 0.0, c); }
        default: { rgb = vec3(c, 0.0, x); }
    }
    return clamp(rgb + vec3(l - c / 2.0), vec3(0.0), vec3(1.0));
}

// `amount` is already the /100 fraction.
fn adjust_lightness(l: f32, amount: f32) -> f32 {
    var v: f32;
    if (amount >= 0.0) {
        v = l + (1.0 - l) * amount;
    } else {
        v = l * (1.0 + amount);
    }
    return clamp(v, 0.0, 1.0);
}

// Photoshop's six-slider mono mix: weight the two colour regions the
// pixel's channel ordering places it between.
fn black_white(base: u32, c: vec3<f32>) -> vec3<f32> {
    let reds = adj_arg(base);
    let yellows = adj_arg(base + 1u);
    let greens = adj_arg(base + 2u);
    let cyans = adj_arg(base + 3u);
    let blues = adj_arg(base + 4u);
    let magentas = adj_arg(base + 5u);
    let r = c.r;
    let g = c.g;
    let b = c.b;
    let mx = max(r, max(g, b));
    let mn = min(r, min(g, b));
    let mid = r + g + b - mx - mn;
    var gray: f32;
    if (mx <= mn + 1e-6) {
        gray = mx;
    } else {
        let t = (mid - mn) / (mx - mn);
        var lo: f32;
        var hi: f32;
        if (r >= g && g >= b) {
            lo = reds;
            hi = yellows;
        } else if (g >= r && r >= b) {
            lo = greens;
            hi = yellows;
        } else if (g >= b && b >= r) {
            lo = greens;
            hi = cyans;
        } else if (b >= g && g >= r) {
            lo = blues;
            hi = cyans;
        } else if (b >= r && r >= g) {
            lo = blues;
            hi = magentas;
        } else {
            lo = reds;
            hi = magentas;
        }
        gray = mn + (mx - mn) * (lo * (1.0 - t) + hi * t);
    }
    let v = clamp(gray, 0.0, 1.0);
    return vec3(v, v, v);
}

fn apply_direct(kind: u32, base: u32, c: vec3<f32>) -> vec3<f32> {
    switch kind {
        case 12u: { return vec3(1.0)-c; }
        case 13u: { return vec3(adj_arg(base),adj_arg(base+1u),adj_arg(base+2u)); }
        case 14u: { return clamp(pow(max(c*adj_arg(base)+vec3(adj_arg(base+1u)),vec3(0.0)),vec3(adj_arg(base+2u))),vec3(0.0),vec3(1.0)); }
        case 15u: { return clamp(((c+vec3(adj_arg(base)))-vec3(0.5))*adj_arg(base+1u)+vec3(0.5),vec3(0.0),vec3(1.0)); }
        case 16u: { var out=c;for(var k=0u;k<3u;k++){out[k]=adj_level(base,adj_level(base+5u*(k+1u),c[k]));}return out; }
        case 17u: {var out=c;for(var k=0u;k<3u;k++){out[k]=adj_curve(base+u32(adj_arg(base)),adj_curve(base+u32(adj_arg(base+k+1u)),c[k]));}return out;}
        case D_HUE_SATURATION: {
            var hue = adj_arg(base);
            var saturation = adj_arg(base + 1u);
            let lightness = adj_arg(base + 2u) / 100.0;
            let colorize = adj_arg(base + 3u) != 0.0;
            let lightness_desaturates = adj_arg(base + 4u) != 0.0;
            let reciprocal_saturation = adj_arg(base + 5u) != 0.0;
            let hsl = rgb_to_hsl(c);
            var range_lightness = 0.0;
            let count = u32(adj_arg(base + 6u));
            for (var i = 0u; i < count; i++) {
                let b = base + 7u + i * 7u;
                let w = adj_hue_weight(b, hsl.x);
                if w > 0.0 {
                    hue += w * adj_arg(b + 4u);
                    saturation += w * adj_arg(b + 5u);
                    range_lightness += w * adj_arg(b + 6u);
                }
            }
            saturation /= 100.0;
            // Affinity's lightness slider flattens colour as it lifts,
            // and its saturation slider boosts reciprocally. Both are
            // off for our own (Photoshop-style) sliders.
            var desat = 1.0;
            if (lightness_desaturates) {
                desat = clamp(1.0 - abs(lightness), 0.0, 1.0);
            }
            var shifted = hsl.y * (1.0 + saturation);
            if (reciprocal_saturation && saturation > 0.0) {
                shifted = hsl.y / max(1.0 - saturation, 0.02);
            }
            var nh: f32;
            var ns: f32;
            if (colorize) {
                nh = rem_euclid_f(hue, 360.0);
                ns = clamp(saturation, 0.0, 1.0);
            } else {
                nh = rem_euclid_f(hsl.x + hue, 360.0);
                ns = clamp(shifted * desat, 0.0, 1.0);
            }
            let rgb = hsl_to_rgb(nh, ns, adjust_lightness(hsl.z, lightness));
            let desired = select(min(rgb.r, min(rgb.g, rgb.b)), max(rgb.r, max(rgb.g, rgb.b)), range_lightness > 0.0);
            return rgb + (vec3(desired) - rgb) * clamp(abs(range_lightness / 100.0), 0.0, 1.0);
        }
        case D_BLACK_WHITE: {
            return black_white(base, c);
        }
        case D_THRESHOLD: {
            let lum = 0.3 * c.r + 0.59 * c.g + 0.11 * c.b;
            if (lum >= adj_arg(base)) {
                return vec3(1.0);
            }
            return vec3(0.0);
        }
        case D_POSTERIZE: {
            // floor into n equal input bands, outputs over the full
            // range — the CPU's (and Photoshop's, and Affinity's)
            // convention.
            let n = adj_arg(base);
            return clamp(
                vec3(
                    min(floor(c.r * n), n - 1.0) / (n - 1.0),
                    min(floor(c.g * n), n - 1.0) / (n - 1.0),
                    min(floor(c.b * n), n - 1.0) / (n - 1.0),
                ),
                vec3(0.0),
                vec3(1.0),
            );
        }
        case D_COLOR_BALANCE: {
            let lum = adj_luma(c);
            let shadow = clamp(1.0 - lum * 2.0, 0.0, 1.0);
            let highlight = clamp((lum - 0.5) * 2.0, 0.0, 1.0);
            let mid = 1.0 - shadow - highlight;
            let shift = (adj_vec3(base) * shadow + adj_vec3(base + 3u) * mid + adj_vec3(base + 6u) * highlight) / 100.0;
            let out = clamp(c + shift, vec3(0.0), vec3(1.0));
            if adj_arg(base + 9u) != 0.0 { return adj_set_lum(out, lum); }
            return out;
        }
        case D_VIBRANCE: { return adj_vibrance(base, c); }
        case D_PHOTO_FILTER: {
            let k = adj_arg(base + 3u);
            let out = clamp(c * (vec3(1.0 - k) + k * adj_vec3(base)), vec3(0.0), vec3(1.0));
            if adj_arg(base + 4u) != 0.0 { return adj_set_lum(out, adj_luma(c)); }
            return out;
        }
        case D_GRADIENT_MAP: {
            var t = adj_luma(c);
            if adj_arg(base) != 0.0 { t = 1.0 - t; }
            let count = u32(adj_arg(base + 1u));
            if count < 2u { return adj_vec3(base + 2u) + (adj_vec3(base + 5u) - adj_vec3(base + 2u)) * t; }
            var lo = base + 8u;
            var hi = lo + (count - 1u) * 4u;
            for (var i = 0u; i + 1u < count; i++) {
                let a = base + 8u + i * 4u;
                if t >= adj_arg(a) && t <= adj_arg(a + 4u) { lo = a; hi = a + 4u; break; }
            }
            let u = clamp((t - adj_arg(lo)) / max(adj_arg(hi) - adj_arg(lo), 0.000001), 0.0, 1.0);
            return adj_vec3(lo + 1u) + (adj_vec3(hi + 1u) - adj_vec3(lo + 1u)) * u;
        }
        case D_SELECTIVE_COLOR: { return adj_selective(base, c); }
        case D_CHANNEL_MIXER: {
            var out = vec3(0.0);
            for (var i = 0u; i < 3u; i++) {
                let row = select(i, 0u, adj_arg(base) != 0.0);
                let v = adj_vec3(base + 1u + row * 3u);
                out[i] = clamp((c.r * v.x + c.g * v.y + c.b * v.z) / 100.0 + adj_arg(base + 10u + row) / 100.0, 0.0, 1.0);
            }
            return out;
        }
        case D_WHITE_BALANCE: {
            let lin = vec3(adj_decode(c.r), adj_decode(c.g), adj_decode(c.b));
            let lms = adj_matvec(base, lin) * adj_vec3(base + 18u);
            let rgb = adj_matvec(base + 9u, lms);
            return vec3(adj_encode(rgb.r), adj_encode(rgb.g), adj_encode(rgb.b));
        }
        default: {
            return c;
        }
    }
}


fn adj_vec3(base: u32) -> vec3<f32> { return vec3(adj_arg(base), adj_arg(base + 1u), adj_arg(base + 2u)); }
fn adj_matvec(base: u32, c: vec3<f32>) -> vec3<f32> {
    var out = vec3(0.0);
    for (var i = 0u; i < 3u; i++) {
        let v = adj_vec3(base + i * 3u);
        out[i] = v.x * c.x + v.y * c.y + v.z * c.z;
    }
    return out;
}
fn adj_hue_weight(base: u32, hue: f32) -> f32 {
    let b0 = adj_arg(base);
    let d = rem_euclid_f(hue - b0, 360.0);
    let up = rem_euclid_f(adj_arg(base + 1u) - b0, 360.0);
    let flat = rem_euclid_f(adj_arg(base + 2u) - b0, 360.0);
    let down = rem_euclid_f(adj_arg(base + 3u) - b0, 360.0);
    if d >= down { return 0.0; }
    if d < up { if up > 0.0 { return d / up; } return 1.0; }
    if d <= flat { return 1.0; }
    if down > flat { return (down - d) / (down - flat); }
    return 1.0;
}
fn adj_luma(c: vec3<f32>) -> f32 { return 0.299 * c.r + 0.587 * c.g + 0.114 * c.b; }
fn adj_set_lum(rgb: vec3<f32>, desired: f32) -> vec3<f32> {
    var c = rgb + vec3(desired - adj_luma(rgb));
    let l = adj_luma(c);
    let lo = min(c.r, min(c.g, c.b));
    if lo < 0.0 && l - lo > 0.000001 { c = vec3(l) + (c - vec3(l)) * l / (l - lo); }
    let hi = max(c.r, max(c.g, c.b));
    if hi > 1.0 && hi - l > 0.000001 { c = vec3(l) + (c - vec3(l)) * (1.0 - l) / (hi - l); }
    return clamp(c, vec3(0.0), vec3(1.0));
}
fn adj_decode(value: f32) -> f32 {
    let v = clamp(value, 0.0, 1.0);
    if v <= 0.04045 { return v / 12.92; }
    return pow((v + 0.055) / 1.055, 2.4);
}
fn adj_encode(value: f32) -> f32 {
    let v = clamp(value, 0.0, 1.0);
    if v <= 0.0031308 { return 12.92 * v; }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}
fn adj_lab_f(t: f32) -> f32 {
    let d = 6.0 / 29.0;
    if t > d * d * d { return pow(t, 1.0 / 3.0); }
    return t / (3.0 * d * d) + 4.0 / 29.0;
}
fn adj_lab_inv(t: f32) -> f32 {
    let d = 6.0 / 29.0;
    if t > d { return t * t * t; }
    return 3.0 * d * d * (t - 4.0 / 29.0);
}
fn adj_vibrance_boost(base: u32, chroma: f32) -> f32 {
    if chroma <= 0.0 { return 0.0; }
    if chroma <= 2.5 { return adj_arg(base + 4u) * (chroma / 2.5); }
    let x = (chroma - 2.5) / 5.0;
    let i = u32(floor(x)) + 1u;
    if i + 1u >= 20u { return adj_arg(base + 22u); }
    let f = x - floor(x);
    return adj_arg(base + 3u + i) * (1.0 - f) + adj_arg(base + 4u + i) * f;
}
fn adj_vibrance(base: u32, c: vec3<f32>) -> vec3<f32> {
    let rgb = vec3(adj_decode(c.r), adj_decode(c.g), adj_decode(c.b));
    let white = vec3(0.4124 + 0.3576 + 0.1805, 0.2126 + 0.7152 + 0.0722, 0.0193 + 0.1192 + 0.9505);
    let xyz = vec3(0.4124 * rgb.r + 0.3576 * rgb.g + 0.1805 * rgb.b,
        0.2126 * rgb.r + 0.7152 * rgb.g + 0.0722 * rgb.b,
        0.0193 * rgb.r + 0.1192 * rgb.g + 0.9505 * rgb.b) / white;
    let f = vec3(adj_lab_f(xyz.x), adj_lab_f(xyz.y), adj_lab_f(xyz.z));
    let l = 116.0 * f.y - 16.0;
    var a = 500.0 * (f.x - f.y);
    var b = 200.0 * (f.y - f.z);
    let t = adj_arg(base);
    var gain = 1.0 + t * 0.5;
    if t > 0.0 {
        var hue = rem_euclid_f(degrees(atan2(b, a)), 360.0);
        if hue > 225.0 { hue -= 360.0; }
        var protect = 0.0;
        if hue < 30.0 { protect = clamp((30.0 - hue) / 45.0, 0.0, 1.0); }
        else if hue > 45.0 { protect = clamp((hue - 45.0) / 45.0, 0.0, 1.0); }
        gain = 1.0 + t * protect * adj_vibrance_boost(base, adj_arg(base + 2u) * length(vec2(a, b)));
    }
    let k = adj_arg(base + 1u) * gain;
    a *= k; b *= k;
    let fy = (l + 16.0) / 116.0;
    let back = vec3(adj_lab_inv(fy + a / 500.0), adj_lab_inv(fy), adj_lab_inv(fy - b / 200.0)) * white;
    let out = vec3(3.24097 * back.x - 1.537383 * back.y - 0.498611 * back.z,
        -0.969244 * back.x + 1.875968 * back.y + 0.041555 * back.z,
        0.05563 * back.x - 0.203977 * back.y + 1.056972 * back.z);
    return vec3(adj_encode(out.r), adj_encode(out.g), adj_encode(out.b));
}
fn adj_selective(base: u32, c: vec3<f32>) -> vec3<f32> {
    let mx = max(c.r, max(c.g, c.b));
    let mn = min(c.r, min(c.g, c.b));
    let mid = c.r + c.g + c.b - mx - mn;
    let sat = mx - mn;
    if sat <= 0.00001 { return c; }
    var primary = 4u; var secondary = 3u;
    if mx == c.r { primary = 0u; secondary = select(5u, 1u, c.g >= c.b); }
    else if mx == c.g { primary = 2u; secondary = select(1u, 3u, c.b >= c.r); }
    else if c.r >= c.g { secondary = 5u; }
    let t = clamp((mid - mn) / sat, 0.0, 1.0);
    let k0 = 1.0 - mx;
    let denom = 1.0 - k0;
    var cmy = vec3(0.0);
    if denom > 0.00001 { cmy = (vec3(1.0) - c - vec3(k0)) / denom; }
    var k = k0;
    for (var i = 0u; i < 6u; i++) {
        var weight = 0.0;
        if i == primary { weight = 1.0 - t; }
        if i == secondary { weight = t; }
        if weight <= 0.0 { continue; }
        let d = adj_vec3(base + 1u + i * 4u) / 100.0 * weight;
        let dk = adj_arg(base + 4u + i * 4u) / 100.0 * weight;
        if adj_arg(base) != 0.0 { cmy += cmy * d; k += k * dk; }
        else { cmy += d; k += dk; }
    }
    return clamp((vec3(1.0) - clamp(cmy, vec3(0.0), vec3(1.0))) * (1.0 - clamp(k, 0.0, 1.0)), vec3(0.0), vec3(1.0));
}

fn adj_level(base:u32,value:f32)->f32 {
    var t=clamp((value-adj_arg(base))/adj_arg(base+1u),0.0,1.0);
    if adj_arg(base+2u)!=1.0 {t=pow(t,adj_arg(base+2u));}
    return clamp(adj_arg(base+3u)+t*adj_arg(base+4u),0.0,1.0);
}
fn adj_curve(base:u32,value:f32)->f32 {
    let n=u32(adj_arg(base));if n==0u {return value;}
    let x=clamp(value,0.0,1.0);
    if x<=adj_arg(base+1u){return clamp(adj_arg(base+2u),0.0,1.0);}
    for(var i=0u;i+1u<n;i++){
        let b=base+1u+i*2u;let x0=adj_arg(b);let y0=adj_arg(b+1u);let x1=adj_arg(b+2u);let y1=adj_arg(b+3u);
        if x>x1{continue;}var t=0.0;if abs(x1-x0)>=0.000001{t=(x-x0)/(x1-x0);}
        if n==2u{return clamp(y0+(y1-y0)*t,0.0,1.0);}
        let prev=adj_arg(base+2u+u32(max(i32(i)-1,0))*2u);let next=adj_arg(base+2u+min(i+2u,n-1u)*2u);
        let t2=t*t;let t3=t2*t;
        return clamp(0.5*((2.0*y0)+(-prev+y1)*t+(2.0*prev-5.0*y0+4.0*y1-next)*t2+(-prev+3.0*y0-3.0*y1+next)*t3),0.0,1.0);
    }
    return clamp(adj_arg(base+2u+(n-1u)*2u),0.0,1.0);
}
