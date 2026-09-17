// Match the CPU border extension, including frames smaller than a CFA period.
fn wrap_cfa(value: i32, count: i32, period: i32) -> i32 {
    var i = value;
    while i < 0 {
        i += period;
    }
    while i >= count {
        i -= period;
    }
    return clamp(i, 0, count - 1);
}

fn compute(i: u32) {
    let x = wrap_cfa(i32(i % shape.width) - 6, i32(args[0]), i32(args[4]));
    let y = wrap_cfa(i32(i / shape.width) + i32(args[2]) - 6, i32(args[1]), i32(args[5]));
    dst[i] = src[u32(y - i32(args[3])) * u32(args[0]) + u32(x)];
}
