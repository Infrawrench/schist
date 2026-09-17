fn compute(i: u32) {
    if args[0] == 0.0 {
        let r = i32(i / shape.width);
        let c = i32(i % shape.width);
        let fw = i32(args[1]);
        let width = i32(args[2]);
        let height = i32(args[3]);
        var x = 0;
        var y = 0;
        var valid = true;
        if args[4] != 0.0 {
            y = r + c - (fw - 1);
            let t = c - r + fw - 1 - (y & 1);
            valid = y >= 0 && y < height && t >= 0 && (t & 1) == 0;
            x = t / 2;
        } else {
            x = c - r + fw - 1;
            let t = r + c - fw + 1 - (x & 1);
            valid = x >= 0 && x < width && t >= 0 && (t & 1) == 0;
            y = t / 2;
        }
        var value = 0.0;
        if valid && x >= 0 && y >= 0 && x < width && y < height {
            value = src[(u32(y) + u32(args[6])) * u32(args[5]) + u32(x) + u32(args[7])];
        }
        dst[i] = value;
        return;
    }
    let p = i / 3u;
    let channel = i % 3u;
    let row = p / shape.width;
    let col = p % shape.width;
    // One-dimensional coordinate tables preserve the reference's f64-to-f32
    // rounding while avoiding a full image of CPU-generated coordinates.
    let r = args[4u + row + shape.width - 1u - col];
    let c = args[4u + u32(args[3]) + row + col];
    let width = u32(args[1]);
    let height = u32(args[2]);
    var value = 0.0;
    if r >= 0.0 && c >= 0.0 && u32(r) + 2u <= height && u32(c) + 2u <= width {
        let at = (u32(r) * width + u32(c)) * 3u + channel;
        let fr = r - f32(u32(r));
        let fc = c - f32(u32(c));
        let top = src[at] * (1.0 - fc) + src[at + 3u] * fc;
        let bottom = src[at + width * 3u] * (1.0 - fc) + src[at + width * 3u + 3u] * fc;
        value = top * (1.0 - fr) + bottom * fr;
    }
    dst[i] = value;
}
