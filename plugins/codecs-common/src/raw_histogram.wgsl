var<workgroup> bins: array<atomic<u32>, 4096>;
fn compute_group(group: u32, lane: u32) {
    for (var b = lane; b < 4096u; b += 256u) {
        atomicStore(&bins[b], 0u);
    }
    workgroupBarrier();
    for (var p = group * 4096u + lane; p < min((group + 1u) * 4096u, u32(args[0])); p += 256u) {
        let i = p * 4u;
        let brightest = clamp(max(max(src[i], src[i + 1u]), src[i + 2u]), 0.0, 1.0);
        atomicAdd(&bins[u32(brightest * 4095.0)], 1u);
    }
    workgroupBarrier();
    for (var b = lane; b < 4096u; b += 256u) {
        dst[group * 4096u + b] = f32(atomicLoad(&bins[b]));
    }
}
