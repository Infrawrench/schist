// args: horizontal/vertical scale, detail, seed, tile, wrap, map width/height, has map.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let p = vec2<f32>(pos) + 0.5;
    var movement = vec2<f32>(0.0);
    if args[8] != 0.0 {
        let size = vec2<u32>(u32(args[6]), u32(args[7]));
        if all(size > vec2<u32>(0u)) {
            var at = p / vec2<f32>(f32(image.width), f32(image.height)) * vec2<f32>(size);
            if args[4] != 0.0 {
                at = p - floor(p / vec2<f32>(size)) * vec2<f32>(size);
            }
            let xy = min(vec2<u32>(at), size - 1u);
            let i = (xy.y * size.x + xy.x) * 4u;
            movement = vec2<f32>(aux[i], aux[i + 1u]);
        }
    } else {
        let seed = u32(args[3]);
        movement = vec2<f32>(fbm(p / args[2], 11u + seed, 3u),
            fbm(p / args[2] + vec2<f32>(37.0, -19.0), 23u + seed, 3u));
    }
    var at = p + (movement - 0.5) * vec2<f32>(args[0], args[1]) * 2.0;
    if args[5] != 0.0 {
        let size = vec2<f32>(f32(image.width), f32(image.height));
        at = at - floor(at / size) * size;
    }
    return straight(sample_premul(at - 0.5));
}
