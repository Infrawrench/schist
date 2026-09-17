fn effect(pos: vec2<i32>) -> vec4<f32> {
    let center = vec2(f32(image.width), f32(image.height)) / 2.0;
    let norm = max(length(center), 1.0);
    let xy = vec2<f32>(pos) + vec2(0.5);
    var out = read_pixel(pos);
    if args[10] != 0.0 {
        let uv = (xy - center) / norm;
        let rotated = vec2(uv.x * args[7] - uv.y * args[6], uv.x * args[6] + uv.y * args[7]) / args[8];
        let plane = rotated / max(1.0 + args[4] * rotated.y + args[5] * rotated.x, 0.05);
        let k = 1.0 - args[0] * dot(plane, plane) * 0.5;
        let mapped = center + plane * k * norm;
        let scales = vec3(args[1], 1.0, args[2]);
        for (var c = 0u; c < 3u; c++) {
            let p = sample_premul(center + (mapped - center) * scales[c] - vec2(0.5));
            out[c] = p[c];
            if c == 1u {
                out.a = p.a;
            }
        }
        out = straight(out);
    }
    if args[3] != 0.0 {
        let r = length((xy - center) / norm);
        let t = clamp((r - args[9]) / max(1.0 - args[9], 0.001), 0.0, 1.0);
        out = vec4(clamp(out.rgb * (1.0 + args[3] * t * t), vec3(0.0), vec3(1.0)), out.a);
    }
    return out;
}
