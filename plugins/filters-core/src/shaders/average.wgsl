fn compute(i: u32) {
    let group = i / 3u;
    let channel = i % 3u;
    let last = min((group + 1u) * 1024u, u32(args[0]));
    var sum = 0.0;
    var correction = 0.0;
    for (var p = group * 1024u; p < last; p++) {
        let value = src[p * 4u + channel] - correction;
        let next = sum + value;
        correction = (next - sum) - value;
        sum = next;
    }
    dst[i] = sum;
}
