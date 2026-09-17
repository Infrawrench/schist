// args: spin, steps, center x/y, then (cos-1, sin) or (scale-1, 0).
// Keep pixel coordinates separate from displacement to preserve fractions.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let centre = vec2<f32>(args[2], args[3]);
    let f = vec2<f32>(pos) + vec2<f32>(0.5) - centre;
    var acc = vec4<f32>(0.0);
    for (var s = 0u; s < u32(args[1]); s++) {
        let a = args[4u + 2u * s];
        let b = args[5u + 2u * s];
        var delta: vec2<f32>;
        if args[0] > 0.0 {
            delta = vec2<f32>(f.x * a - f.y * b, f.x * b + f.y * a);
        } else {
            delta = f * a;
        }
        acc += sample_premul_offset(pos, delta) / args[1];
    }
    return straight(acc);
}
