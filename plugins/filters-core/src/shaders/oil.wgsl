// args: radius, levels (<=64), bristle, shine, light cosine, light sine.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let r = i32(args[0]);
    let levels = u32(args[1]);
    var counts: array<u32, 64>;
    var sums: array<vec3<f32>, 64>;
    for (var dy = -r; dy <= r; dy++) {
        for (var dx = -r; dx <= r; dx++) {
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let p = read_pixel(pos + vec2<i32>(dx, dy));
            let bin = u32(clamp(luminance(p) * args[1], 0.0, args[1] - 1.0));
            counts[bin]++;
            sums[bin] += p.rgb;
        }
    }
    var best = 0u;
    for (var i = 1u; i < levels; i++) {
        if counts[i] >= counts[best] {
            best = i;
        }
    }
    var rgb = sums[best] / f32(max(counts[best], 1u));
    if args[2] > 0.0 {
        let gx = luminance(read_pixel(pos + vec2<i32>(1, 0))) - luminance(read_pixel(pos - vec2<i32>(1, 0)));
        let gy = luminance(read_pixel(pos + vec2<i32>(0, 1))) - luminance(read_pixel(pos - vec2<i32>(0, 1)));
        let across = f32(pos.x) * -gy + f32(pos.y) * gx;
        let comb = sin(across * 2.0) * args[2] * 0.05;
        let facing = gx * args[4] + gy * args[5];
        let lit = 1.0 + comb + facing * args[3] * 2.0;
        rgb = clamp(rgb * lit, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return vec4<f32>(rgb, read_pixel(pos).a);
}
