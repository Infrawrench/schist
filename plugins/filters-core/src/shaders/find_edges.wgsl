// Sobel magnitude in CPU tap order. No parameters.
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let kx = array<f32,9>(-1.0,0.0,1.0,-2.0,0.0,2.0,-1.0,0.0,1.0);
    let ky = array<f32,9>(-1.0,-2.0,-1.0,0.0,0.0,0.0,1.0,2.0,1.0);
    var gx = 0.0;
    var gy = 0.0;
    for (var i = 0u; i < 9u; i++) {
        let l = luminance(read_pixel(pos + vec2<i32>(i32(i%3u)-1, i32(i/3u)-1)));
        gx += l * kx[i];
        gy += l * ky[i];
    }
    let v = clamp(1.0 - length(vec2<f32>(gx, gy)), 0.0, 1.0);
    return vec4<f32>(v, v, v, read_pixel(pos).a);
}
