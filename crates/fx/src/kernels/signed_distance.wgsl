fn compute(i: u32) {
    let w = i32(shape.width);
    let h = i32(shape.height);
    let x = i32(i % shape.width);
    let y = i32(i / shape.width);
    let inside = src[i] >= 0.5;
    let r = i32(max(ceil(args[0]), 1.0));
    var best = args[0];
    for (var dy = -r; dy <= r; dy++) {
        let sy = y + dy;
        if sy < 0 || sy >= h {
            continue;
        }
        for (var dx = -r; dx <= r; dx++) {
            let sx = x + dx;
            if sx < 0 || sx >= w {
                continue;
            }
            if (src[u32(sy * w + sx)] >= 0.5) == inside {
                continue;
            }
            best = min(best, sqrt(f32(dx * dx + dy * dy)));
            if best <= 1.0 {
                break;
            }
        }
        if best <= 1.0 {
            break;
        }
    }
    best = max(best - 0.5, 0.0);
    dst[i] = select(best, -best, inside);
}
