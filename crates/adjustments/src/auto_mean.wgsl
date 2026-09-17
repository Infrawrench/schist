var<workgroup> sums: array<vec4<f32>, 256>;

fn compute_group(block: u32, lane: u32) {
    let range = bounds();
    let span = max(range[1] - range[0], vec3(0.0001));
    var sum = vec4(0.0);
    for (var offset = lane; offset < 4096u; offset += 256u) {
        let pixel = block * 4096u + offset;
        if pixel < shape.width && src[pixel * 4u + 3u] > 0.0 {
            let c = vec3(src[pixel * 4u], src[pixel * 4u + 1u], src[pixel * 4u + 2u]);
            sum += vec4(clamp((c - range[0]) / span, vec3(0.0), vec3(1.0)), 1.0);
        }
    }
    sums[lane] = sum;
    workgroupBarrier();
    for (var stride = 128u; stride > 0u; stride /= 2u) {
        if lane < stride {
            sums[lane] += sums[lane + stride];
        }
        workgroupBarrier();
    }
    if lane < 4u {
        dst[block * 4u + lane] = sums[0][lane];
    }
}
