// args: level, upper side (1) or lower side (0).
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let p = read_pixel(pos);
    let r = read_pixel(pos + vec2<i32>(1, 0));
    let d = read_pixel(pos + vec2<i32>(0, 1));
    var out = vec4<f32>(1.0, 1.0, 1.0, p.a);
    for (var c = 0u; c < 3u; c++) {
        let here = p[c] < args[0];
        let crosses = here != (r[c] < args[0]) || here != (d[c] < args[0]);
        if crosses && (here != (args[1] > 0.0)) {
            out[c] = 0.0;
        }
    }
    return out;
}
