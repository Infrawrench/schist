fn compute(i: u32) {
    if args[0] == 0.0 {
        var sum = 0.0;
        for (var group = 0u; group < u32(args[1]); group++) {
            sum += src[group * 4096u + i];
        }
        dst[i] = sum;
        return;
    }
    if args[0] == 1.0 {
        var seen = 0u;
        var bin = 4095u;
        for (var b = 0u; b < 4096u; b++) {
            seen += u32(src[b]);
            if seen * 100u >= u32(args[1]) * 99u {
                bin = b;
                break;
            }
        }
        var gain = 1.0;
        if bin > 0u {
            gain = clamp(4095.0 / f32(bin), 1.0, 4.0);
        }
        dst[0u] = gain * args[2];
        return;
    }
    if i % 4u == 3u {
        dst[i] = src[i];
        return;
    }
    var v = max(src[i] * aux[0u], 0.0);
    let knee = 0.85;
    if v > knee {
        v = knee + (1.0 - knee) * (1.0 - exp(-(v - knee) / (1.0 - knee)));
    }
    dst[i] = args[1u + u32(min(v, 1.0) * 65535.0 + 0.5)];
}
