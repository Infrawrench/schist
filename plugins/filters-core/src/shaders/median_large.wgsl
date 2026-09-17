// Radix selection has constant scratch space for every radius.
fn ordered(v: f32) -> u32 {
    let u = bitcast<u32>(v);
    return select(u ^ 0x80000000u, ~u, (u & 0x80000000u) != 0u);
}

fn effect(pos: vec2<i32>) -> vec4<f32> {
    let r = i32(args[0]);
    var out = read_pixel(pos);
    var count = 0u;
    for (var y = -r; y <= r; y++) {
        for (var x = -r; x <= r; x++) {
            if args[1] > 0.0 && x * x + y * y > r * r {
                continue;
            }
            count++;
        }
    }
    for (var c = 0u; c < u32(args[2]); c++) {
        var prefix = 0u;
        var mask = 0u;
        var rank = count / 2u;
        for (var bit = 31; bit >= 0; bit--) {
            let flag = 1u << u32(bit);
            var zeros = 0u;
            for (var y = -r; y <= r; y++) {
                for (var x = -r; x <= r; x++) {
                    if args[1] > 0.0 && x * x + y * y > r * r {
                        continue;
                    }
                    let key = ordered(read_pixel(pos + vec2(x, y))[c]);
                    if (key & mask) == prefix && (key & flag) == 0u {
                        zeros++;
                    }
                }
            }
            if rank >= zeros {
                prefix |= flag;
                rank -= zeros;
            }
            mask |= flag;
        }
        let bits = select(~prefix, prefix ^ 0x80000000u, (prefix & 0x80000000u) != 0u);
        let median = bitcast<f32>(bits);
        if abs(out[c] - median) > args[3] {
            out[c] = median;
        }
    }
    return out;
}
