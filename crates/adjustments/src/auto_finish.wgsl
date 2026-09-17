fn compute(channel: u32) {
    let range = bounds();
    var gamma = 1.0;
    if args[0] == 2.0 {
        var total = 0.0;
        // Compensated addition keeps large images close to the f64 CPU mean.
        var correction = 0.0;
        var count = 0.0;
        for (var block = 0u; block < shape.width; block++) {
            let value = src[block * 4u + channel] - correction;
            let next = total + value;
            correction = (next - total) - value;
            total = next;
            count += src[block * 4u + 3u];
        }
        if count > 0.0 {
            let mean = total / count;
            if mean > 0.001 && mean < 0.999 {
                gamma = clamp(log(0.5) / log(mean), 0.2, 5.0);
            }
        }
    }
    dst[channel] = range[0][channel];
    dst[channel + 3u] = range[1][channel];
    dst[channel + 6u] = gamma;
}
