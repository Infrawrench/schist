// args: side, scale, bias, skip_zero, row-major weights.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let side = u32(args[0]);
    var acc = vec3<f32>(0.0);
    for (var i = 0u; i < side * side; i++) {
        let weight = args[4u + i];
        if args[3] > 0.0 && weight == 0.0 { continue; }
        let p = read_pixel(pos + vec2<i32>(i32(i % side), i32(i / side)) - vec2<i32>(i32(side / 2u)));
        acc += p.rgb * weight;
    }
    return vec4<f32>(clamp(acc / args[1] + vec3<f32>(args[2]), vec3<f32>(0.0), vec3<f32>(1.0)), read_pixel(pos).a);
}
