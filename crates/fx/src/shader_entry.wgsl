@compute @workgroup_size(16, 16)
fn run_effect(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= image.width || id.y >= image.rows {
        return;
    }
    destination[id.y * image.width + id.x] = effect(vec2<i32>(i32(id.x), i32(id.y + image.first_row)));
}
