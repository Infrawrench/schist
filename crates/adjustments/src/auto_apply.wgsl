fn compute(pixel: u32) {
    let base = pixel * 4u;
    let alpha = src[base + 3u];
    for (var channel = 0u; channel < 3u; channel++) {
        var value = src[base + channel];
        if alpha > 0.0 {
            value = clamp((value - aux[channel]) / max(aux[channel + 3u] - aux[channel], 0.0001), 0.0, 1.0);
            let gamma = aux[channel + 6u];
            if gamma != 1.0 {
                value = pow(value, gamma);
            }
        }
        dst[base + channel] = value;
    }
    dst[base + 3u] = alpha;
}
