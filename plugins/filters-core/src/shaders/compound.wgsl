fn compute(i: u32) {
    let mode = u32(args[0]);
    let c = i % 4u;
    let pixel = i / 4u;
    if mode <= 1u {
        let original = aux[i];
        if c == 3u {
            dst[i] = original;
            return;
        }
        if mode == 0u {
            dst[i] = clamp(original - src[i] + 0.5, 0.0, 1.0);
            return;
        }
        let diff = original - src[i];
        dst[i] = original;
        if abs(diff) >= args[3] {
            dst[i] = clamp(original + diff * args[2], 0.0, 1.0);
        }
        return;
    }
    let x = f32(pixel % shape.width);
    let y = f32(pixel / shape.width);
    let w = f32(shape.width);
    let h = f32(shape.height);
    var amount = 0.0;
    if mode == 2u {
        let dx = args[3];
        let dy = args[4];
        let extent = abs(w * dx) + abs(h * dy);
        amount = abs(x * dx + y * dy - extent * args[5]) / max(extent * args[6], 1.0);
    } else if mode == 3u {
        let reach = min(w, h) * args[5];
        let rx = reach * (1.0 + (1.0 - args[6]) * max(w / max(h, 1.0) - 1.0, 0.0));
        let ry = reach * (1.0 + (1.0 - args[6]) * max(h / max(w, 1.0) - 1.0, 0.0));
        let d = length(vec2((x - args[3] * w) / max(rx, 1.0), (y - args[4] * h) / max(ry, 1.0)));
        amount = (d - (1.0 - args[7])) / max(args[7], 0.001);
    } else {
        let dx = args[3];
        let dy = args[4];
        let extent = abs(w * dx) + abs(h * dy);
        let along = abs(x * dx + y * dy - extent * args[5]);
        let band = extent * args[6] / 2.0;
        let feather = extent * args[7];
        amount = select(0.0, 1.0, along > band);
        if feather > 0.0001 {
            amount = (along - band) / feather;
        }
    }
    let t = clamp(amount, 0.0, 1.0) * 3.0;
    let stage = args[2];
    let weight = clamp(1.0 - abs(t - (stage + 1.0)), 0.0, 1.0);
    var value = aux[i];
    if stage == 0.0 {
        value *= clamp(1.0 - t, 0.0, 1.0);
    }
    dst[i] = value + src[i] * weight;
}
