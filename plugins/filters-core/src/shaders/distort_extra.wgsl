// Range reduction keeps the alternating series below tan(pi/8).
fn precise_atan(x: f32) -> f32 {
    var t = abs(x);
    var offset = 0.0;
    var reciprocal = false;
    if t > 1.0 {
        t = 1.0 / t;
        reciprocal = true;
    }
    if t > 0.414213562373095 {
        t = (t - 1.0) / (t + 1.0);
        offset = 0.7853981633974483;
    }
    let square = t * t;
    var term = t;
    var sum = t;
    for (var k = 1u; k < 12u; k++) {
        term *= -square;
        sum += term / f32(2u * k + 1u);
    }
    var a = offset + sum;
    if reciprocal {
        a = 1.5707963267948966 - a;
    }
    return select(a, -a, x < 0.0);
}

fn precise_atan2(y: f32, x: f32) -> f32 {
    if x == 0.0 {
        return select(select(0.0, 1.5707963267948966, y > 0.0), -1.5707963267948966, y < 0.0);
    }
    var a = precise_atan(y / x);
    if x < 0.0 {
        a += select(-3.141592653589793, 3.141592653589793, y >= 0.0);
    }
    return a;
}

const PI: f32 = 3.141592653589793;
const TAU: f32 = 6.283185307179586;
fn mapped(p: vec2<f32>) -> vec2<f32> {
    let mode = u32(args[0]);
    let size = vec2<f32>(f32(image.width), f32(image.height));
    let centre = size / 2.0;
    let delta = p - centre;
    let radius = max(length(centre), 1.0);
    let d = length(delta);
    if mode == 0u {
        if d < 0.001 {
            return p;
        }
        let phase = d / radius * args[2] * TAU;
        if args[3] == 0.0 {
            let t = sin(phase) * args[1] * 0.6 * max(1.0 - d / radius, 0.0);
            return centre + vec2(delta.x * cos(t) - delta.y * sin(t), delta.x * sin(t) + delta.y * cos(t));
        }
        var push = sin(phase) * args[1] * radius * 0.1;
        if args[3] == 1.0 {
            push = abs(sin(phase)) * args[1] * radius * 0.1;
        } else {
            push *= max(1.0 - d / radius, 0.0);
        }
        return p + delta / d * push;
    }
    if mode == 1u || mode == 2u {
        var v = delta;
        if args[2] == 1.0 {
            v.y = 0.0;
        }
        if args[2] == 2.0 {
            v.x = 0.0;
        }
        let reach = max(min(centre.x, centre.y), 1.0);
        if mode == 1u {
            // Avoid cancellation in 1-t*t after rounding the normalized radius.
            let squared = dot(v, v);
            let radius_squared = reach * reach;
            if squared >= radius_squared || squared < 0.000001 {
                return p;
            }
            let dist = sqrt(squared);
            let height = sqrt(radius_squared - squared);
            let bulged = clamp(precise_atan2(dist, height) / (PI / 2.0), 0.0, 1.0);
            let scale = 1.0 + (bulged / (dist / reach) - 1.0) * args[1];
            return centre + v * scale;
        }
        let dist = length(v);
        if dist >= reach || dist < 0.001 {
            return p;
        }
        let t = dist / reach;
        let scale = pow(t, 1.0 + args[1]) / t;
        return centre + v * scale;
    }
    if mode == 3u {
        if args[1] >= 0.5 {
            return vec2((precise_atan2(delta.y, delta.x) + PI) / TAU * size.x, d / radius * size.y);
        }
        let theta = p.x / size.x * TAU - PI;
        let r = p.y / size.y * radius;
        return centre + r * vec2(cos(theta), sin(theta));
    }
    if mode == 4u {
        let t = p.y / size.y;
        var shift = sin(t * PI);
        if args[2] == 1.0 {
            shift = sin(t * TAU);
        }
        if args[2] == 2.0 {
            shift = t * 2.0 - 1.0;
        }
        var x = p.x + shift * args[1];
        if args[3] >= 0.5 {
            x = x - floor(x / size.x) * size.x;
        }
        return vec2(x, p.y);
    }
    if mode == 5u {
        let uv = delta / radius / args[2];
        let r = length(uv);
        if r < 0.000001 {
            return p;
        }
        let theta = precise_atan(r * args[3]);
        var rs = theta;
        if args[1] == 1.0 {
            rs = tan(theta);
        }
        if args[1] == 2.0 {
            rs = 2.0 * sin(theta / 2.0);
        }
        return centre + uv / r * (rs / args[4]) * radius;
    }
    return p;
}

fn effect(pos: vec2<i32>) -> vec4<f32> {
    return straight(sample_premul(mapped(vec2<f32>(pos) + vec2(0.5)) - vec2(0.5)));
}
