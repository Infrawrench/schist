// Tensor kernels use NCHW. Model compilation validates dimensions/operators first.
fn compute(i: u32) {
    let op = u32(args[0]);
    if op == 0u {
        let ic = u32(args[1]);
        let ih = u32(args[2]);
        let iw = u32(args[3]);
        let oc = u32(args[4]);
        let oh = u32(args[5]);
        let ow = u32(args[6]);
        let kh = u32(args[7]);
        let kw = u32(args[8]);
        let groups = u32(args[15]);
        let in_group = ic / groups;
        let out_group = oc / groups;
        let x = i % ow;
        let y = (i / ow) % oh;
        let c = (i / (ow * oh)) % oc;
        let batch = i / (ow * oh * oc);
        let group = c / out_group;
        var sum = aux[oc * in_group * kh * kw + c];
        for (var k = 0u; k < in_group; k++) {
            for (var ky = 0u; ky < kh; ky++) {
                for (var kx = 0u; kx < kw; kx++) {
                    let sy = i32(y * u32(args[9]) + ky * u32(args[11])) - i32(args[13]);
                    let sx = i32(x * u32(args[10]) + kx * u32(args[12])) - i32(args[14]);
                    if sx < 0 || sy < 0 || sx >= i32(iw) || sy >= i32(ih) {
                        continue;
                    }
                    let a = ((batch * ic + group * in_group + k) * ih + u32(sy)) * iw + u32(sx);
                    let b = ((c * in_group + k) * kh + ky) * kw + kx;
                    sum += src[a] * aux[b];
                }
            }
        }
        dst[i] = sum;
        return;
    }
    if op == 1u {
        dst[i] = max(src[i], 0.0);
        return;
    }
    if op == 2u {
        dst[i] = select(src[i] * args[1], src[i], src[i] >= 0.0);
        return;
    }
    if op == 3u || op == 4u || op == 13u {
        let rank = u32(args[1]);
        var a = 0u;
        var b = 0u;
        var index = i;
        for (var k = i32(rank) - 1; k >= 0; k--) {
            let d = u32(args[2u + u32(k) * 3u]);
            let coordinate = index % d;
            index /= d;
            a += coordinate * u32(args[3u + u32(k) * 3u]);
            b += coordinate * u32(args[4u + u32(k) * 3u]);
        }
        if op == 13u {
            dst[i] = src[a] / aux[b];
        } else {
            dst[i] = select(src[a] + aux[b], src[a] * aux[b], op == 4u);
        }
        return;
    }
    if op == 5u {
        dst[i] = 1.0 / (1.0 + exp(-src[i]));
        return;
    }
    if op == 6u {
        dst[i] = tanh(src[i]);
        return;
    }
    if op == 7u {
        dst[i] = clamp(src[i], args[1], args[2]);
        return;
    }
    if op == 8u {
        let m = u32(args[1]);
        let k = u32(args[2]);
        let n = u32(args[3]);
        let row = i / n;
        let col = i % n;
        var sum = 0.0;
        for (var j = 0u; j < k; j++) {
            sum += src[row * k + j] * aux[j * n + col];
        }
        dst[i] = sum;
        return;
    }
    if op == 9u {
        let rank = u32(args[1]);
        var offset = 0u;
        var index = i;
        for (var k = i32(rank) - 1; k >= 0; k--) {
            let d = u32(args[2u + u32(k) * 2u]);
            offset += (index % d) * u32(args[3u + u32(k) * 2u]);
            index /= d;
        }
        dst[i] = src[offset];
        return;
    }
    if op == 10u {
        let spatial = u32(args[1]);
        let channels = u32(args[2]);
        let c = (i / spatial) % channels;
        dst[i] = src[i] * aux[c] + aux[channels + c];
        return;
    }
    if op == 11u {
        let iw = u32(args[1]);
        let ih = u32(args[2]);
        let ow = u32(args[3]);
        let oh = u32(args[4]);
        let x = i % ow;
        let y = (i / ow) % oh;
        let plane = i / (ow * oh);
        let sx = min(u32(floor(f32(x) / args[5])), iw - 1u);
        let sy = min(u32(floor(f32(y) / args[6])), ih - 1u);
        dst[i] = src[(plane * ih + sy) * iw + sx];
        return;
    }
    if op == 12u {
        let inner = u32(args[1]);
        let a = u32(args[2]) * inner;
        let b = u32(args[3]) * inner;
        let outer = i / (a + b);
        let offset = i % (a + b);
        if offset < a {
            dst[i] = src[outer * a + offset];
        } else {
            dst[i] = aux[outer * b + offset - a];
        }
        return;
    }
    if op == 14u {
        let n = u32(args[1]);
        let stride = u32(args[2]);
        let base = (i / (n * stride)) * (n * stride) + i % stride;
        var top = src[base];
        for (var j = 1u; j < n; j++) {
            top = max(top, src[base + j * stride]);
        }
        var sum = 0.0;
        for (var j = 0u; j < n; j++) {
            sum += exp(src[base + j * stride] - top);
        }
        dst[i] = exp(src[i] - top) / sum;
        return;
    }
    dst[i] = src[i];
}
