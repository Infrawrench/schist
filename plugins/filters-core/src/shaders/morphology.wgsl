// args: radius, maximum, disc. Includes alpha, as the CPU does.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let r = i32(args[0]);
    let take_max = args[1] > 0.0;
    var acc = vec4<f32>(select(1.0, 0.0, take_max));
    for (var dy = -r; dy <= r; dy++) {
        for (var dx = -r; dx <= r; dx++) {
            if args[2] > 0.0 && dx * dx + dy * dy > r * r { continue; }
            let p = read_pixel(pos + vec2<i32>(dx, dy));
            acc = select(min(acc, p), max(acc, p), take_max);
        }
    }
    return acc;
}
