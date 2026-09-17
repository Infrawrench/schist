fn compute(i: u32) {
    var delta = 0.0;
    let channels = select(3u, 4u, args[0] == 2.0);
    for (var c = 0u; c < channels; c++) {
        var value = src[i * 4u + c];
        if args[0] == 2.0 {
            value = floor(clamp(value, 0.0, 1.0) * 255.0 + 0.5);
        }
        delta = max(delta, abs(value - args[1u + c]));
    }
    var coverage = select(0.0, 1.0, delta <= args[5]);
    if args[0] == 0.0 {
        coverage = select(0.0, 1.0, delta == 0.0);
        if args[5] > 0.0 {
            coverage = clamp(1.0 - delta / args[5], 0.0, 1.0);
        }
    }
    if args[6] != 0.0 && aux[i] > 0.0 {
        coverage = 1.0;
    }
    dst[i] = coverage;
}
