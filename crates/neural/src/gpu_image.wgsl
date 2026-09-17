fn mirror(v: i32, n: i32) -> u32 {
    if n <= 1 {
        return 0u;
    }
    let period = 2 * (n - 1);
    var m = ((v % period) + period) % period;
    if m >= n {
        m = period - m;
    }
    return u32(m);
}

fn compute(i: u32) {
    let w = u32(args[1]);
    let h = u32(args[2]);
    let t = u32(args[3]);
    let overlap = u32(args[4]);
    let x0 = u32(args[5]);
    let y0 = u32(args[6]);
    if args[0] == 0.0 {
        let c = i / (t * t);
        let p = i % (t * t);
        let x = mirror(i32(x0 + p % t) - i32(overlap), i32(w));
        let y = mirror(i32(y0 + p / t) - i32(overlap), i32(h));
        let v = src[(y * w + x) * 4u + c];
        // Byte-range models multiply by 255 rather than divide by its rounded reciprocal.
        if args[17u] != 0.0 {
            dst[i] = v * 255.0;
        } else {
            dst[i] = (v - args[11u + c]) / args[14u + c];
        }
        return;
    }
    let p = i / 4u;
    let c = i % 4u;
    let x = p % w;
    let y = p / w;
    let step = u32(args[7]);
    var value = src[i];
    if c < 3u && x >= x0 && x < x0 + step && y >= y0 && y < y0 + step {
        let ow = u32(args[8]);
        let oh = u32(args[9]);
        let sx = min(x - x0 + overlap, ow - 1u);
        let sy = min(y - y0 + overlap, oh - 1u);
        var v = aux[(c * oh + sy) * ow + sx];
        if args[17u] != 0.0 {
            v /= 255.0;
        } else {
            v = v * args[14u + c] + args[11u + c];
        }
        value += (clamp(v, 0.0, 1.0) - value) * args[10];
    }
    dst[i] = value;
}
