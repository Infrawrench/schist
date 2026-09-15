// args: tap count, followed by precomputed floating point (dx, dy).
// Preparing offsets on the CPU preserves its angle and rounding semantics.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    var acc = vec4<f32>(0.0);
    var count = 0.0;
    for (var i = 0u; i < u32(args[0]); i++) {
        let q = vec2<i32>(round_away(vec2<f32>(pos) + vec2<f32>(args[1u + 2u*i], args[2u + 2u*i])));
        if any(q < vec2<i32>(0)) || q.x >= i32(image.width) || q.y >= i32(image.height) { continue; }
        acc += premul(read_pixel(q));
        count += 1.0;
    }
    return straight(acc / count);
}
