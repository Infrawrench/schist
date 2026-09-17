// Parents use positive, exactly representable float indices (one based), so
// subsequent ordinary kernels can read the same buffer without denormals.
// Zero means the node is its own root. Every link decreases the root index.
fn root(start: u32) -> u32 {
    var node = start;
    loop {
        let bits = atomicLoad(&dst[node]);
        if bits == 0u {
            break;
        }
        node = u32(bitcast<f32>(bits)) - 1u;
    }
    return node;
}

fn unite(left: u32, right: u32) {
    loop {
        let a = root(left);
        let b = root(right);
        if a == b {
            break;
        }
        let high = max(a, b);
        let low = min(a, b);
        let result = atomicCompareExchangeWeak(&dst[high], 0u, bitcast<u32>(f32(low + 1u)));
        if result.exchanged {
            break;
        }
    }
}

fn compute(i: u32) {
    if args[0] == 1.0 {
        if aux[i] > 0.0 && src[i] > 0.0 {
            atomicStore(&dst[u32(src[i]) - 1u], bitcast<u32>(1.0));
        }
        return;
    }
    if src[i] <= 0.0 {
        return;
    }
    if i % shape.width > 0u && src[i - 1u] > 0.0 {
        unite(i, i - 1u);
    }
    if i >= shape.width && src[i - shape.width] > 0.0 {
        unite(i, i - shape.width);
    }
}
