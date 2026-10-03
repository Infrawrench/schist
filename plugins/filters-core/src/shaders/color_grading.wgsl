// Camera Raw colour grading: mirrors `color_grading::Grading::apply`.
// args: edge, exponent, four RGB tints (shadows, midtones, highlights,
// global), then the four luminance shifts.

fn grade_tint(k: u32) -> vec3<f32> {
    return vec3<f32>(args[2u + k * 3u], args[3u + k * 3u], args[4u + k * 3u]);
}

fn grade_smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn effect(pos: vec2<i32>) -> vec4<f32> {
    let p = read_pixel(pos);
    let edge = args[0];
    let t = pow(clamp(luminance(p), 0.0, 1.0), args[1]);
    var shadows = 1.0 - grade_smooth(1.0 / 3.0 - edge, 1.0 / 3.0 + edge, t);
    var highlights = grade_smooth(2.0 / 3.0 - edge, 2.0 / 3.0 + edge, t);
    let sum = shadows + highlights;
    if sum > 1.0 {
        shadows /= sum;
        highlights /= sum;
    }
    let w = vec4<f32>(shadows, 1.0 - shadows - highlights, highlights, 1.0);
    var lum = 0.0;
    var shift = vec3<f32>(0.0);
    for (var k = 0u; k < 4u; k++) {
        lum += w[k] * args[14u + k];
        shift += w[k] * grade_tint(k);
    }
    var c = p.rgb;
    if lum >= 0.0 {
        c = c + lum * (vec3<f32>(1.0) - c);
    } else {
        c = c + lum * c;
    }
    return vec4<f32>(clamp(c + shift, vec3<f32>(0.0), vec3<f32>(1.0)), p.a);
}
