// args: integer light-direction x/y steps, amount.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let offset = vec2<i32>(i32(args[0]), -i32(args[1]));
    let a = luminance(read_pixel(pos - offset));
    let b = luminance(read_pixel(pos + offset));
    let g = clamp(0.5 + (b-a) * args[2], 0.0, 1.0);
    return vec4<f32>(g, g, g, read_pixel(pos).a);
}
