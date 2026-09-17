// The slider's sweep gives sample angles in [-pi/6, pi/6]. This series
// avoids backend-dependent native trig precision; larger API inputs retain sin.
fn spin_sine(a: f32) -> f32 {
    if abs(a) > 0.6 {
        return sin(a);
    }
    let square = a * a;
    return a * (1.0 + square * (-1.0 / 6.0
        + square * (1.0 / 120.0 + square * (-1.0 / 5040.0 + square / 362880.0))));
}

// mode 0: spin; 1: path (precomputed 24 offset/weight triples); 2: shape; 3: smart.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let mode = u32(args[0]);
    let here = read_pixel(pos);
    var sum = vec4(0.0);
    var total = 0.0;
    if mode == 0u {
        let centre = vec2(args[2] * f32(image.width), args[3] * f32(image.height));
        let reach = f32(max(image.width, image.height)) * args[4];
        if args[1] <= 0.0 || reach <= 0.0 {
            return here;
        }
        let p = vec2<f32>(pos) + vec2(0.5) - centre;
        let r = length(p);
        let inside = 1.0 - clamp((r / reach - (1.0 - args[5])) / args[5], 0.0, 1.0);
        if inside <= 0.001 {
            return straight(premul(here));
        }
        for (var s = 0u; s < 24u; s++) {
            let t = f32(s) / 23.0 - 0.5;
            let a = t * args[1] * inside;
            let sine = spin_sine(a);
            let half_sine = spin_sine(a * 0.5);
            let cosine_minus_one = -2.0 * half_sine * half_sine;
            let offset = vec2(p.x * cosine_minus_one - p.y * sine, p.x * sine + p.y * cosine_minus_one);
            sum += sample_premul_offset(pos, offset) / 24.0;
        }
        return straight(sum);
    }
    if mode == 1u {
        if args[1] <= 0.0 {
            return here;
        }
        for (var s = 0u; s < 24u; s++) {
            let b = 2u + s * 3u;
            let weight = args[b + 2u];
            sum += sample_premul(vec2<f32>(pos) + vec2(args[b], args[b + 1u])) * weight;
            total += weight;
        }
        return straight(sum / max(total, 0.000001));
    }
    if mode == 2u {
        let n = u32(args[1]);
        if n == 0u {
            return here;
        }
        for (var k = 0u; k < n; k++) {
            sum += premul(read_pixel(pos + vec2<i32>(i32(args[2u + k * 2u]), i32(args[3u + k * 2u])))) / f32(n);
        }
        return straight(sum);
    }
    let r = i32(args[1]);
    for (var y = -r; y <= r; y++) {
        for (var x = -r; x <= r; x++) {
            if x * x + y * y > r * r {
                continue;
            }
            let p = read_pixel(pos + vec2(x, y));
            let d = abs(p.rgb - here.rgb);
            if max(d.x, max(d.y, d.z)) > args[2] {
                continue;
            }
            sum += p;
            total += 1.0;
        }
    }
    let n = max(total, 1.0);
    let smoothed = sum.rgb / n;
    let edge = min(clamp(1.0 - n / f32((2 * r + 1) * (2 * r + 1)), 0.0, 1.0) * 2.5, 1.0);
    if args[3] == 1.0 {
        return vec4(vec3(edge), here.a);
    }
    if args[3] == 2.0 {
        return vec4(min(smoothed + vec3(edge), vec3(1.0)), here.a);
    }
    return vec4(smoothed, here.a);
}
