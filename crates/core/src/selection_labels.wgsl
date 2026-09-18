fn compute(i: u32) {
    if args[0] == 1.0 {
        dst[i] = 0.0;
        if src[i] > 0.0 {
            dst[i] = aux[u32(src[i]) - 1u];
        }
        return;
    }
    if aux[i] <= 0.0 {
        dst[i] = 0.0;
        return;
    }
    var parent = i;
    if src[i] > 0.0 {
        parent = u32(src[i]) - 1u;
    }
    var ancestor = parent;
    if src[parent] > 0.0 {
        ancestor = u32(src[parent]) - 1u;
    }
    dst[i] = f32(ancestor + 1u);
}
