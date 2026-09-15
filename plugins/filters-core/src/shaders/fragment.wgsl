// args: integer diagonal displacement.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let d = i32(args[0]);
    var acc = vec4<f32>(0.0);
    let offsets = array<vec2<i32>,4>(vec2<i32>(-d,-d),vec2<i32>(d,-d),vec2<i32>(-d,d),vec2<i32>(d,d));
    for (var i = 0u; i < 4u; i++) { acc += premul(read_pixel(pos + offsets[i])) / 4.0; }
    return straight(acc);
}
