// Most common 3x3 neighbor; ties keep the first candidate. No parameters.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    var neighbours: array<vec4<f32>,9>;
    for (var i = 0u; i < 9u; i++) { neighbours[i] = read_pixel(pos + vec2<i32>(i32(i%3u)-1, i32(i/3u)-1)); }
    var best = read_pixel(pos);
    var best_count = 0u;
    for (var i = 0u; i < 9u; i++) {
        var count = 0u;
        for (var j = 0u; j < 9u; j++) {
            if all(abs(neighbours[j].rgb - neighbours[i].rgb) < vec3<f32>(0.06)) { count++; }
        }
        if count > best_count { best_count = count; best = neighbours[i]; }
    }
    return best;
}
