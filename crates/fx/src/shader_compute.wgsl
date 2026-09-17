// The same effect body can run as one stage in a resident compute graph.
struct Image {
    width: u32,
    height: u32,
    first_row: u32,
    rows: u32,
}

var<private> image: Image;

fn read_pixel(pos: vec2<i32>) -> vec4<f32> {
    let p = clamp(pos, vec2<i32>(0), vec2<i32>(i32(shape.width) - 1, i32(shape.height) - 1));
    let i = (u32(p.y) * shape.width + u32(p.x)) * 4u;
    return vec4<f32>(src[i], src[i + 1u], src[i + 2u], src[i + 3u]);
}

@compute @workgroup_size(256)
fn run_compute(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = gid.x + gid.y * groups.x * 256u;
    if i >= shape.len {
        return;
    }
    image = Image(shape.width, shape.height, 0u, shape.height);
    let p = effect(vec2<i32>(i32(i % shape.width), i32(i / shape.width)));
    for (var c = 0u; c < 4u; c++) {
        dst[i * 4u + c] = p[c];
    }
}
