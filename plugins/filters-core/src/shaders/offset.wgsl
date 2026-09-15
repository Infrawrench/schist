// args: integer x/y offset, undefined area mode (transparent/edge/wrap).
fn effect(pos: vec2<i32>) -> vec4<f32> {
    var q = pos - vec2<i32>(i32(args[0]), i32(args[1]));
    let size = vec2<i32>(i32(image.width), i32(image.height));
    if args[2] == 0.0 && (any(q < vec2<i32>(0)) || any(q >= size)) { return vec4<f32>(0.0); }
    if args[2] == 2.0 { q = ((q % size) + size) % size; }
    return read_pixel(q);
}
