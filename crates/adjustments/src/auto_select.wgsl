fn compute(quantile: u32) {
    let digit = u32(args[0]);
    var counts: array<u32, 256>;
    var total = 0u;
    for (var bin = 0u; bin < 256u; bin++) {
        var count = 0u;
        for (var block = 0u; block < shape.width; block++) {
            count += u32(src[(block * 6u + quantile) * 256u + bin]);
        }
        counts[bin] = count;
        total += count;
    }
    var rank = 0u;
    var prefix = 0u;
    var visible = total;
    if digit == 0u {
        if total > 0u {
            rank = min(u32(f32(total) * select(0.005, 0.995, quantile % 2u == 1u)), total - 1u);
        }
    } else {
        rank = u32(aux[quantile * 4u + 2u]);
        prefix = u32(aux[quantile * 4u]) | (u32(aux[quantile * 4u + 1u]) << 16u);
        visible = u32(aux[quantile * 4u + 3u]);
    }
    var chosen = 0u;
    for (var bin = 0u; bin < 256u; bin++) {
        if counts[bin] > rank {
            chosen = bin;
            break;
        }
        rank -= counts[bin];
    }
    let key = (prefix << 8u) | chosen;
    dst[quantile * 4u] = f32(key & 65535u);
    dst[quantile * 4u + 1u] = f32(key >> 16u);
    dst[quantile * 4u + 2u] = f32(rank);
    dst[quantile * 4u + 3u] = f32(visible);
}
