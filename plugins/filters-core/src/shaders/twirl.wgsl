// args: angle in radians, center x/y, radius.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let centre = vec2<f32>(args[1],args[2]);
    let p = vec2<f32>(pos) + vec2<f32>(0.5);
    let d = p - centre;
    let radius = args[3];
    var delta = vec2<f32>(0.0);
    let distance = length(d);
    if distance < radius {
        let falloff = 1.0 - distance / radius;
        let t = args[0] * (falloff * falloff);
        let s = sin(t); let c = cos(t);
        delta = vec2<f32>(d.x*(c - 1.0) - d.y*s, d.x*s + d.y*(c - 1.0));
    }
    return straight(sample_premul_offset(pos, delta));
}
