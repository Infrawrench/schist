fn sample(x: i32, y: i32) -> f32 {
    if x < 0 || y < 0 || x >= i32(shape.width) || y >= i32(shape.height) { return 0.0; }
    return src[u32(y) * shape.width + u32(x)];
}
fn compute(i: u32) {
    let x = f32(i % shape.width) - args[0]; let y = f32(i / shape.width) - args[1];
    let x0 = i32(floor(x)); let y0 = i32(floor(y));
    let fx = x - f32(x0); let fy = y - f32(y0);
    dst[i] = sample(x0,y0)*(1.0-fx)*(1.0-fy) + sample(x0+1,y0)*fx*(1.0-fy)
        + sample(x0,y0+1)*(1.0-fx)*fy + sample(x0+1,y0+1)*fx*fy;
}
