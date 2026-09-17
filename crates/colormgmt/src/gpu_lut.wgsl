fn compute(i: u32) {
    let inputs = u32(args[0]);
    let outputs = u32(args[1]);
    let grid = u32(args[2]);
    let pixel = i / u32(args[4]);
    let channel = i % u32(args[4]);
    if channel >= outputs {
        dst[i] = select(1.0, src[pixel * u32(args[3]) + 3u], args[5] != 0.0);
        return;
    }
    var coords: array<f32, 4>;
    for (var c = 0u; c < inputs; c++) {
        let value = clamp(src[pixel * u32(args[3]) + c], 0.0, 1.0);
        coords[c] = floor(value * 255.0 + 0.5) / 255.0 * f32(grid - 1u);
    }
    var result = 0.0;
    for (var corner = 0u; corner < (1u << inputs); corner++) {
        var index = 0u;
        var weight = 1.0;
        for (var c = 0u; c < inputs; c++) {
            let fraction = coords[c] - floor(coords[c]);
            let high = (corner & (1u << c)) != 0u;
            index = index * grid + min(u32(coords[c]) + select(0u, 1u, high), grid - 1u);
            weight *= select(1.0 - fraction, fraction, high);
        }
        result += aux[index * outputs + channel] * weight;
    }
    if args[6] != 0.0 {
        result = clamp(result, 0.0, 1.0);
    }
    dst[i] = result;
}
