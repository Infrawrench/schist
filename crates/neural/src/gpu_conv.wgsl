// 16 x 16 implicit matrix multiplication. Reuse both image samples and weights
// within each workgroup; the image stays in NCHW without an im2col allocation.
var<workgroup> weights: array<f32, 256>;
var<workgroup> pixels: array<f32, 256>;
fn compute_group(group: u32, lane: u32) {
    let ci = u32(args[0]); let ih = u32(args[1]); let iw = u32(args[2]);
    let co = u32(args[3]); let oh = u32(args[4]); let ow = u32(args[5]);
    let kh = u32(args[6]); let kw = u32(args[7]);
    let row = lane / 16u; let col = lane % 16u;
    let columns = (oh * ow + 15u) / 16u;
    let c = (group / columns) * 16u + row;
    let p = (group % columns) * 16u + col;
    let count = ci * kh * kw;
    var sum = 0.0;
    for (var base = 0u; base < count; base += 16u) {
        let wk = base + col;
        weights[lane] = 0.0;
        if c < co && wk < count { weights[lane] = aux[c * count + wk]; }
        let k = base + row;
        pixels[lane] = 0.0;
        if p < oh * ow && k < count {
            let y = i32((p / ow) * u32(args[8]) + (k / kw % kh) * u32(args[10])) - i32(args[12]);
            let x = i32((p % ow) * u32(args[9]) + (k % kw) * u32(args[11])) - i32(args[13]);
            if y >= 0 && x >= 0 && y < i32(ih) && x < i32(iw) {
                pixels[lane] = src[((k / (kh * kw)) * ih + u32(y)) * iw + u32(x)];
            }
        }
        workgroupBarrier();
        for (var k = 0u; k < 16u; k++) { sum += weights[row * 16u + k] * pixels[k * 16u + col]; }
        workgroupBarrier();
    }
    if c < co && p < oh * ow { dst[c * oh * ow + p] = sum + aux[co * count + c]; }
}
