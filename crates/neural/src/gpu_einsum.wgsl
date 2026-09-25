fn compute(i: u32) {
    let rank = u32(args[0]); let reductions = u32(args[1]);
    var index = i; var a = 0u; var b = 0u;
    for (var d = i32(rank) - 1; d >= 0; d--) {
        let p = 3u + u32(d) * 3u; let n = u32(args[p]);
        let coordinate = index % n; index /= n;
        a += coordinate * u32(args[p + 1u]); b += coordinate * u32(args[p + 2u]);
    }
    var sum = 0.0;
    for (var k = 0u; k < u32(args[2]); k++) {
        var remaining = k; var ai = a; var bi = b;
        for (var d = i32(reductions) - 1; d >= 0; d--) {
            let p = 3u + (rank + u32(d)) * 3u; let n = u32(args[p]);
            let coordinate = remaining % n; remaining /= n;
            ai += coordinate * u32(args[p + 1u]); bi += coordinate * u32(args[p + 2u]);
        }
        sum += src[ai] * aux[bi];
    }
    dst[i] = sum;
}
