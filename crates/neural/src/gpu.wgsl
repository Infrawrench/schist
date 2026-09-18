fn resize_coordinate(value: f32, scale: f32, original: f32, output: f32) -> f32 {
    let mode = u32(args[7]);
    if mode == 0u {
        return value / scale;
    }
    if mode == 2u && output <= 1.0 {
        return 0.0;
    }
    if mode == 3u {
        return select(0.0, value * (original - 1.0) / max(output - 1.0, 1.0), output > 1.0);
    }
    if mode == 4u {
        return (value + 0.5) / scale;
    }
    var offset = 0.0;
    if mode == 5u {
        offset = original / 2.0 * (1.0 - output / (original * scale));
    }
    return offset + (value + 0.5) / scale - 0.5;
}

fn resize_sample(pos: vec2<i32>, plane: u32) -> f32 {
    let p = clamp(pos, vec2<i32>(0), vec2<i32>(i32(args[1]) - 1, i32(args[2]) - 1));
    return src[(plane * u32(args[2]) + u32(p.y)) * u32(args[1]) + u32(p.x)];
}

fn cubic_weight(value: f32) -> f32 {
    let x = abs(value);
    let a = args[10];
    if x <= 1.0 {
        return (a + 2.0) * x * x * x - (a + 3.0) * x * x + 1.0;
    }
    if x < 2.0 {
        return a * x * x * x - 5.0 * a * x * x + 8.0 * a * x - 4.0 * a;
    }
    return 0.0;
}

// Tensor kernels use NCHW. Model compilation validates dimensions/operators first.
fn compute(i: u32) {
    let op = u32(args[0]);
    if op == 27u {
        let count = u32(args[1]);
        let start = (i / 2u) * count;
        var mean = 0.0;
        var correction = 0.0;
        for (var k = 0u; k < count; k++) {
            let value = src[start + k] - correction;
            let next = mean + value;
            correction = (next - mean) - value;
            mean = next;
        }
        mean /= f32(count);
        if i % 2u == 0u {
            dst[i] = mean;
            return;
        }
        var variance = 0.0;
        correction = 0.0;
        for (var k = 0u; k < count; k++) {
            let d = src[start + k] - mean;
            let value = d * d - correction;
            let next = variance + value;
            correction = (next - variance) - value;
            variance = next;
        }
        dst[i] = variance / f32(count);
        return;
    }
    if op == 28u {
        let group = i / u32(args[1]);
        let c = group % u32(args[2]);
        dst[i] = (src[i] - aux[group * 2u]) / sqrt(aux[group * 2u + 1u] + args[3]) * args[4u + c] + args[4u + u32(args[2]) + c];
        return;
    }
    if op == 29u {
        let m = u32(args[1]);
        let n = u32(args[2]);
        let k = u32(args[3]);
        let row = i / n;
        let col = i % n;
        var sum = 0.0;
        for (var d = 0u; d < k; d++) {
            let a = select(row * k + d, d * m + row, args[4] != 0.0);
            let b = select(d * n + col, col * k + d, args[5] != 0.0);
            sum += src[a] * aux[b];
        }
        let bias = (row % u32(args[8])) * u32(args[9]) + col % u32(args[9]);
        dst[i] = sum * args[6] + aux[k * n + bias] * args[7];
        return;
    }
    if op == 24u {
        let inner = u32(args[1]);
        let old = u32(args[2]);
        let start = u32(args[3]);
        let count = u32(args[4]);
        dst[i] = src[(i / (count * inner) * old + start) * inner + i % (count * inner)];
        return;
    }
    if op == 25u {
        var rem = i;
        var index = 0;
        for (var axis = i32(args[1]) - 1; axis >= 0; axis--) {
            let p = 2u + u32(axis) * 4u;
            let dim = u32(args[p]);
            index += (i32(rem % dim) * i32(args[p + 3u]) + i32(args[p + 2u])) * i32(args[p + 1u]);
            rem /= dim;
        }
        dst[i] = src[u32(index)];
        return;
    }
    if op == 26u {
        let inner = u32(args[1]);
        let dim = u32(args[2]);
        let count = u32(args[3]);
        let selected = u32(args[4u + (i / inner) % count]);
        dst[i] = src[(i / (inner * count) * dim + selected) * inner + i % inner];
        return;
    }
    if op == 0u || op == 16u {
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
                    var sy = i32(y * u32(args[9]) + ky * u32(args[11])) - i32(args[13]);
                    var sx = i32(x * u32(args[10]) + kx * u32(args[12])) - i32(args[14]);
                    if op == 16u {
                        sy = i32(y) + i32(args[13]) - i32(ky * u32(args[11]));
                        sx = i32(x) + i32(args[14]) - i32(kx * u32(args[12]));
                        if sy < 0 || sx < 0 || sy % i32(args[9]) != 0 || sx % i32(args[10]) != 0 {
                            continue;
                        }
                        sy /= i32(args[9]);
                        sx /= i32(args[10]);
                    }
                    if sx < 0 || sy < 0 || sx >= i32(iw) || sy >= i32(ih) {
                        continue;
                    }
                    let a = ((batch * ic + group * in_group + k) * ih + u32(sy)) * iw + u32(sx);
                    var b = ((c * in_group + k) * kh + ky) * kw + kx;
                    if op == 16u {
                        b = (((group * in_group + k) * out_group + c % out_group) * kh + ky) * kw + kx;
                    }
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
    if op == 3u || op == 4u || op == 13u || (op >= 20u && op <= 23u) {
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
        if op == 20u {
            dst[i] = src[a] - aux[b];
        } else if op == 21u {
            let x = src[a];
            let exponent = aux[b];
            var value = pow(abs(x), exponent);
            if x < 0.0 {
                if exponent != trunc(exponent) {
                    // WGSL constant evaluation rejects NaN. Keep its payload
                    // dependent on the runtime input for undefined real powers.
                    value = bitcast<f32>(0x7fc00000u | (bitcast<u32>(x) & 0x003fffffu));
                } else if abs(exponent) % 2.0 == 1.0 {
                    value = -value;
                }
            }
            dst[i] = value;
        } else if op == 22u {
            dst[i] = min(src[a], aux[b]);
        } else if op == 23u {
            dst[i] = max(src[a], aux[b]);
        } else if op == 13u {
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
        let row = (i / n) % m;
        let col = i % n;
        var batch = i / (m * n);
        var a_offset = 0u;
        var b_offset = 0u;
        for (var dim = i32(args[4]) - 1; dim >= 0; dim--) {
            let base = 5u + u32(dim) * 3u;
            let coordinate = batch % u32(args[base]);
            batch /= u32(args[base]);
            a_offset += coordinate * u32(args[base + 1u]);
            b_offset += coordinate * u32(args[base + 2u]);
        }
        var sum = 0.0;
        for (var j = 0u; j < k; j++) {
            sum += src[a_offset + row * k + j] * aux[b_offset + j * n + col];
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
        let xy = vec2<f32>(resize_coordinate(f32(x), args[5], args[1], args[3]), resize_coordinate(f32(y), args[6], args[2], args[4]));
        if args[8] == 0.0 {
            var nearest = floor(xy);
            if args[9] == 1.0 {
                nearest = ceil(xy);
            } else if args[9] == 2.0 {
                nearest = ceil(xy - 0.5);
            } else if args[9] == 3.0 {
                nearest = floor(xy + 0.5);
            }
            dst[i] = resize_sample(vec2<i32>(nearest), plane);
        } else if args[8] == 1.0 {
            let base = floor(xy);
            let t = xy - base;
            let pos = vec2<i32>(base);
            let top = resize_sample(pos, plane) * (1.0 - t.x) + resize_sample(pos + vec2<i32>(1, 0), plane) * t.x;
            let bottom = resize_sample(pos + vec2<i32>(0, 1), plane) * (1.0 - t.x) + resize_sample(pos + vec2<i32>(1, 1), plane) * t.x;
            dst[i] = top * (1.0 - t.y) + bottom * t.y;
        } else {
            let base = vec2<i32>(floor(xy));
            var total = 0.0;
            var sum = 0.0;
            for (var dy = -1; dy <= 2; dy++) {
                for (var dx = -1; dx <= 2; dx++) {
                    let pos = base + vec2<i32>(dx, dy);
                    if args[11] != 0.0 && (any(pos < vec2<i32>(0)) || pos.x >= i32(iw) || pos.y >= i32(ih)) {
                        continue;
                    }
                    let weight = cubic_weight(xy.x - f32(pos.x)) * cubic_weight(xy.y - f32(pos.y));
                    sum += resize_sample(pos, plane) * weight;
                    total += weight;
                }
            }
            dst[i] = sum / total;
        }
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
    if op == 15u {
        let rank = u32(args[1]);
        var index = i;
        var offset = 0u;
        var outside = false;
        for (var k = i32(rank) - 1; k >= 0; k--) {
            let base = 4u + u32(k) * 4u;
            let dim = u32(args[base]);
            let original = i32(args[base + 1u]);
            var coordinate = i32(index % dim) - i32(args[base + 3u]);
            index /= dim;
            if coordinate < 0 || coordinate >= original {
                outside = true;
                if args[2] == 2.0 && original > 1 {
                    let period = 2 * (original - 1);
                    coordinate = ((coordinate % period) + period) % period;
                    coordinate = min(coordinate, period - coordinate);
                } else {
                    coordinate = clamp(coordinate, 0, original - 1);
                }
            }
            offset += u32(coordinate) * u32(args[base + 2u]);
        }
        dst[i] = select(src[offset], args[3], outside && args[2] == 0.0);
        return;
    }
    if op == 17u {
        let rank = u32(args[2]);
        var output = i;
        var base = 0u;
        for (var k = i32(rank) - 1; k >= 0; k--) {
            let at = 4u + u32(k) * 3u;
            if args[at + 2u] == 0.0 {
                base += (output % u32(args[at])) * u32(args[at + 1u]);
                output /= u32(args[at]);
            }
        }
        var sum = 0.0;
        if args[1] == 2.0 {
            sum = -3.402823e38;
        }
        if args[1] == 3.0 {
            sum = 3.402823e38;
        }
        for (var j = 0u; j < u32(args[3]); j++) {
            var reduced = j;
            var offset = base;
            for (var k = i32(rank) - 1; k >= 0; k--) {
                let at = 4u + u32(k) * 3u;
                if args[at + 2u] != 0.0 {
                    offset += (reduced % u32(args[at])) * u32(args[at + 1u]);
                    reduced /= u32(args[at]);
                }
            }
            let value = src[offset];
            if args[1] == 2.0 {
                sum = max(sum, value);
            } else if args[1] == 3.0 {
                sum = min(sum, value);
            } else if args[1] >= 4.0 {
                sum += value * value;
            } else {
                sum += value;
            }
        }
        if args[1] == 0.0 {
            sum /= args[3];
        }
        if args[1] == 4.0 {
            sum = sqrt(sum);
        }
        dst[i] = sum;
        return;
    }
    if op == 18u {
        let ow = u32(args[4]);
        let oh = u32(args[5]);
        let x = i % ow;
        let y = (i / ow) % oh;
        let plane = i / (ow * oh);
        var sum = select(0.0, -3.402823e38, args[1] == 1.0);
        var count = 0.0;
        for (var ky = 0; ky < i32(args[7]); ky++) {
            for (var kx = 0; kx < i32(args[6]); kx++) {
                let sx = i32(x) * i32(args[8]) + kx * i32(args[10]) - i32(args[12]);
                let sy = i32(y) * i32(args[9]) + ky * i32(args[11]) - i32(args[13]);
                if sx < 0 || sy < 0 || sx >= i32(args[2]) || sy >= i32(args[3]) {
                    if args[14] != 0.0 {
                        count += 1.0;
                    }
                    continue;
                }
                let value = src[(plane * u32(args[3]) + u32(sy)) * u32(args[2]) + u32(sx)];
                if args[1] == 1.0 {
                    sum = max(sum, value);
                } else {
                    sum += value;
                }
                count += 1.0;
            }
        }
        if args[1] == 0.0 {
            sum /= max(count, 1.0);
        }
        dst[i] = sum;
        return;
    }
    if op == 19u {
        let value = src[i];
        switch u32(args[1]) {
            case 0u: {
                dst[i] = sqrt(value);
            }
            case 1u: {
                dst[i] = exp(value);
            }
            case 2u: {
                dst[i] = log(value);
            }
            case 3u: {
                dst[i] = abs(value);
            }
            case 4u: {
                dst[i] = -value;
            }
            case 5u: {
                dst[i] = 1.0 / value;
            }
            case 6u: {
                let x = abs(value);
                let t = 1.0 / (1.0 + 0.3275911 * x);
                let poly = (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t;
                dst[i] = sign(value) * (1.0 - poly * exp(-x * x));
            }
            case 7u: {
                dst[i] = clamp(args[2] * value + args[3], 0.0, 1.0);
            }
            default: {
                dst[i] = value * clamp(value / 6.0 + 0.5, 0.0, 1.0);
            }
        }
        return;
    }
    dst[i] = src[i];
}
