// Immutable candidate patches are auxiliary input; only the target changes.
fn compute(i: u32) {
    let count = u32(args[1]);
    if args[0] == 0.0 {
        var score = 0.0;
        for (var j = 0u; j < 49u; j++) {
            let offset = (i * 49u + j) * 3u;
            let delta = vec3<f32>(aux[offset], aux[offset + 1u], aux[offset + 2u]) - vec3<f32>(src[j * 4u], src[j * 4u + 1u], src[j * 4u + 2u]);
            score += src[j * 4u + 3u] * (delta.x * delta.x + delta.y * delta.y + delta.z * delta.z);
        }
        dst[i] = score;
        return;
    }
    // Stable first-candidate tie breaking, matching the CPU's ordered reduction.
    var best = 0u;
    var low = src[0u];
    for (var candidate = 1u; candidate < count; candidate++) {
        if src[candidate] < low {
            low = src[candidate];
            best = candidate;
        }
    }
    dst[0u] = f32(best);
}
