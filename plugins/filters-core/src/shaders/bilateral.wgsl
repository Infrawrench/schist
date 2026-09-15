// args: radius, colour threshold, disc (Surface Blur) / square (Reduce Noise).
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let r = i32(args[0]);
    let centre = read_pixel(pos);
    var acc = vec4<f32>(0.0);
    var total = 0.0;
    for (var dy = -r; dy <= r; dy++) {
        for (var dx = -r; dx <= r; dx++) {
            if args[2] > 0.0 && dx * dx + dy * dy > r * r { continue; }
            let p = read_pixel(pos + vec2<i32>(dx, dy));
            let delta = abs(p.rgb - centre.rgb);
            let d = max(max(delta.r, delta.g), delta.b);
            if args[2] > 0.0 && d > args[1] { continue; }
            let k = max(1.0 - d / args[1], 0.0);
            acc += p * k;
            total += k;
        }
    }
    if total > 0.0 { return acc / total; }
    return centre;
}
