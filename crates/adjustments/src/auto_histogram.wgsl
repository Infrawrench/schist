// One block owns 4096 pixels. Six radix histograms select the two exact
// percentiles of each channel; no quantization of HDR or negative samples.
var<workgroup> histogram: array<atomic<u32>, 1536>;

fn ordered(value: f32) -> u32 {
    let bits = bitcast<u32>(value);
    return select(bits ^ 0x80000000u, ~bits, (bits & 0x80000000u) != 0u);
}

fn compute_group(block: u32, lane: u32) {
    for (var quantile = 0u; quantile < 6u; quantile++) {
        atomicStore(&histogram[quantile * 256u + lane], 0u);
    }
    workgroupBarrier();
    let digit = u32(args[0]);
    let shift = 24u - digit * 8u;
    for (var offset = lane; offset < 4096u; offset += 256u) {
        let pixel = block * 4096u + offset;
        if pixel >= shape.width || src[pixel * 4u + 3u] <= 0.0 {
            continue;
        }
        for (var quantile = 0u; quantile < 6u; quantile++) {
            let key = ordered(src[pixel * 4u + quantile / 2u]);
            if digit > 0u {
                let prefix = u32(aux[quantile * 4u]) | (u32(aux[quantile * 4u + 1u]) << 16u);
                if (key >> (shift + 8u)) != prefix {
                    continue;
                }
            }
            atomicAdd(&histogram[quantile * 256u + ((key >> shift) & 255u)], 1u);
        }
    }
    workgroupBarrier();
    for (var quantile = 0u; quantile < 6u; quantile++) {
        dst[(block * 6u + quantile) * 256u + lane] = f32(atomicLoad(&histogram[quantile * 256u + lane]));
    }
}
