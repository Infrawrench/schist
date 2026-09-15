// args: generators, amplitude, horizontal, vertical, kind, then (k, phase).
fn shape(phase: f32) -> f32 {
    let tau = 6.283185307179586;
    var t = phase % tau;
    if t < 0.0 { t += tau; }
    t /= tau;
    if args[4] == 1.0 { return 4.0 * abs(t - 0.5) - 1.0; }
    if args[4] == 2.0 { return select(-1.0, 1.0, t < 0.5); }
    return sin(phase);
}
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let p = vec2<f32>(pos) + vec2<f32>(0.5);
    var offset = vec2<f32>(0.0);
    for (var g = 0u; g < u32(args[0]); g++) {
        let k = args[5u+2u*g]; let phase = args[6u+2u*g];
        offset += vec2<f32>(shape(p.y*k+phase), shape(p.x*k+phase)) * args[1] / args[0];
    }
    return straight(sample_premul_offset(pos, offset * vec2<f32>(args[2],args[3])));
}
