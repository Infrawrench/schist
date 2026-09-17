fn adj_arg(i: u32) -> f32 {
    return args[i];
}

fn compute(i: u32) {
    if i >= shape.width {
        return;
    }
    let b = i * 4u;
    let c = apply_direct(u32(args[0]), 1u, vec3(src[b], src[b + 1u], src[b + 2u]));
    dst[b] = c.r;
    dst[b + 1u] = c.g;
    dst[b + 2u] = c.b;
    dst[b + 3u] = src[b + 3u];
}
