fn bound(index: u32) -> f32 {
    let key = u32(aux[index * 4u]) | (u32(aux[index * 4u + 1u]) << 16u);
    let bits = select(~key, key ^ 0x80000000u, (key & 0x80000000u) != 0u);
    return bitcast<f32>(bits);
}

fn bounds() -> mat2x3<f32> {
    var lo = vec3(bound(0u), bound(2u), bound(4u));
    var hi = vec3(bound(1u), bound(3u), bound(5u));
    if args[0] == 1.0 {
        lo = vec3(min(lo.x, min(lo.y, lo.z)));
        hi = vec3(max(hi.x, max(hi.y, hi.z)));
    }
    return mat2x3(lo, hi);
}
