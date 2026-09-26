// Implicit 32 x 64 GEMM, with a 2 x 4 register tile per thread. Image samples
// are shared across output channels without allocating a full im2col tensor.
var<workgroup> weights: array<f32, 512>;
var<workgroup> pixels: array<f32, 1024>;
fn compute_group(group: u32, lane: u32) {
    let ci = u32(args[0]); let ih = u32(args[1]); let iw = u32(args[2]);
    let co = u32(args[3]); let oh = u32(args[4]); let ow = u32(args[5]);
    let kh = u32(args[6]); let kw = u32(args[7]);
    let row = lane / 16u; let col = lane % 16u;
    let columns = (oh * ow + 63u) / 64u;
    let c = group / columns * 32u + row;
    let p = group % columns * 64u + col;
    let count = ci * kh * kw;
    var s0 = vec2<f32>(0.0); var s1 = vec2<f32>(0.0);
    var s2 = vec2<f32>(0.0); var s3 = vec2<f32>(0.0);
    for (var base = 0u; base < count; base += 16u) {
        let wk = base + col;
        for (var j = 0u; j < 2u; j++) {
            weights[lane + j * 256u] = 0.0;
            if c + j * 16u < co && wk < count {
                weights[lane + j * 256u] = aux[(c + j * 16u) * count + wk];
            }
        }
        let k = base + row;
        for (var j = 0u; j < 4u; j++) {
            let pos = p + j * 16u;
            let index = row * 64u + col + j * 16u;
            pixels[index] = 0.0;
            if pos < oh * ow && k < count {
                let y = i32((pos / ow) * u32(args[8]) + (k / kw % kh) * u32(args[10])) - i32(args[12]);
                let x = i32((pos % ow) * u32(args[9]) + (k % kw) * u32(args[11])) - i32(args[13]);
                if y >= 0 && x >= 0 && y < i32(ih) && x < i32(iw) {
                    pixels[index] = src[((k / (kh * kw)) * ih + u32(y)) * iw + u32(x)];
                }
            }
        }
        workgroupBarrier();
        for (var k = 0u; k < 16u; k++) {
            let a = row * 16u + k;
            let b = k * 64u + col;
            let w = vec2<f32>(weights[a], weights[a + 256u]);
            s0 += w * pixels[b];
            s1 += w * pixels[b + 16u];
            s2 += w * pixels[b + 32u];
            s3 += w * pixels[b + 48u];
        }
        workgroupBarrier();
    }
    for (var j = 0u; j < 2u; j++) {
        if c + j * 16u < co {
            let out = (c + j * 16u) * oh * ow + p;
            let bias = aux[co * count + c + j * 16u];
            if p < oh * ow { dst[out] = s0[j] + bias; }
            if p + 16u < oh * ow { dst[out + 16u] = s1[j] + bias; }
            if p + 32u < oh * ow { dst[out + 32u] = s2[j] + bias; }
            if p + 48u < oh * ow { dst[out + 48u] = s3[j] + bias; }
        }
    }
}
