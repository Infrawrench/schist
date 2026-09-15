// args: radius (1..4), disc, channel count, threshold (-1 = always).
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let r = i32(args[0]);
    var out = read_pixel(pos);
    for (var c = 0u; c < u32(args[2]); c++) {
        var values: array<f32, 81>;
        var n = 0u;
        for (var dy = -r; dy <= r; dy++) {
            for (var dx = -r; dx <= r; dx++) {
                if args[1] > 0.0 && dx * dx + dy * dy > r * r { continue; }
                let v = read_pixel(pos + vec2<i32>(dx, dy))[c];
                var j = n;
                loop {
                    if j == 0u { break; }
                    if values[j - 1u] <= v { break; }
                    values[j] = values[j - 1u];
                    j--;
                }
                values[j] = v;
                n++;
            }
        }
        let median = values[n / 2u];
        if abs(out[c] - median) > args[3] { out[c] = median; }
    }
    return out;
}
