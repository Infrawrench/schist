fn effect(pos: vec2<i32>) -> vec4<f32> {
    let winner = u32(aux[u32(pos.y) * image.width + u32(pos.x)]);
    if winner == 0u {
        return vec4<f32>(vec3<f32>(0.0), read_pixel(pos).a);
    }
    let i = winner - 1u;
    let size = u32(args[0]);
    let cols = (image.width + size - 1u) / size;
    let block = i / (size * size);
    let x = i % size;
    let y = i / size % size;
    let base = vec2<u32>(block % cols, block / cols) * size;
    var point = base + vec2<u32>(x, y);
    if args[3] >= 0.5 {
        point = base + vec2<u32>(size / 2u);
    }
    let p = read_pixel(vec2<i32>(point));
    var shade = 1.0;
    if args[4] >= 0.5 {
        shade = 1.0 - abs(f32(x) / f32(size) - 0.5 + f32(y) / f32(size) - 0.5) * 0.9;
    }
    return vec4<f32>(clamp(p.rgb * shade, vec3<f32>(0.0), vec3<f32>(1.0)), p.a);
}
