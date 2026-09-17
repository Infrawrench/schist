// args: radius, mode (0 contract, 1 expand, 2 majority), canvas bounds relative to the plane.
fn compute(i: u32) {
    let x = i32(i % shape.width); let y = i32(i / shape.width);
    let w = i32(shape.width); let h = i32(shape.height);
    let r = i32(args[0]); let mode = u32(args[1]);
    var hits = 0u; var count = 0u;
    for (var dy = -r; dy <= r; dy++) {
        for (var dx = -r; dx <= r; dx++) {
            if dx * dx + dy * dy > r * r { continue; }
            count++;
            let sx = x + dx; let sy = y + dy;
            var inside = false;
            if sx >= 0 && sy >= 0 && sx < w && sy < h { inside = src[u32(sy * w + sx)] >= 128.0; }
            if mode != 2u && (sx < i32(args[2]) || sy < i32(args[3]) || sx >= i32(args[4]) || sy >= i32(args[5])) { inside = mode == 0u; }
            if mode == 1u && inside { dst[i] = 255.0; return; }
            if mode == 0u && !inside { dst[i] = 0.0; return; }
            if inside { hits++; }
        }
    }
    dst[i] = select(0.0, 255.0, mode == 0u || (mode == 2u && hits > count / 2u));
}
