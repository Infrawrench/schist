// Cooperative 64 x 64 contraction with 256 threads, each accumulating a 4 x 4
// register tile. Reuse input values across 16 products per thread and keep the
// shared-memory footprint at 8 KiB, including on mobile/WebGPU adapters.
var<workgroup> left_tile: array<f32, 1024>;
var<workgroup> right_tile: array<f32, 1024>;

fn offsets(flat: u32, start: u32, count: u32) -> vec3<u32> {
    var index = flat;
    var result = vec3<u32>(0u);
    for (var d = i32(count) - 1; d >= 0; d--) {
        let p = 8u + (start + u32(d)) * 4u;
        let dim = u32(args[p]);
        let coordinate = index % dim;
        index /= dim;
        result += coordinate * vec3<u32>(u32(args[p + 1u]), u32(args[p + 2u]), u32(args[p + 3u]));
    }
    return result;
}

fn compute_group(group: u32, lane: u32) {
    let m = u32(args[0]); let n = u32(args[1]); let k_count = u32(args[2]);
    let rows = u32(args[5]); let cols = u32(args[6]); let batches = u32(args[7]);
    let column_tiles = (n + 63u) / 64u;
    let tiles_per_batch = ((m + 63u) / 64u) * column_tiles;
    let tile = group % tiles_per_batch;
    let row = tile / column_tiles * 64u + lane / 16u;
    let col = tile % column_tiles * 64u + lane % 16u;
    let batch_offset = offsets(group / tiles_per_batch, rows + cols, batches);
    var row_offsets: array<vec3<u32>, 4>;
    var col_offsets: array<vec3<u32>, 4>;
    for (var j = 0u; j < 4u; j++) {
        row_offsets[j] = offsets(row + j * 16u, 0u, rows);
        col_offsets[j] = offsets(col + j * 16u, rows, cols);
    }
    var s0 = vec4<f32>(0.0); var s1 = vec4<f32>(0.0);
    var s2 = vec4<f32>(0.0); var s3 = vec4<f32>(0.0);
    for (var base = 0u; base < k_count; base += 16u) {
        let ak = base + lane % 16u;
        let bk = base + lane / 16u;
        let ao = offsets(ak, u32(args[4]), u32(args[3]));
        let bo = offsets(bk, u32(args[4]), u32(args[3]));
        for (var j = 0u; j < 4u; j++) {
            let ai = lane + j * 256u;
            let bi = lane / 16u * 64u + lane % 16u + j * 16u;
            left_tile[ai] = 0.0;
            right_tile[bi] = 0.0;
            if row + j * 16u < m && ak < k_count {
                left_tile[ai] = src[batch_offset.x + row_offsets[j].x + ao.x];
            }
            if col + j * 16u < n && bk < k_count {
                right_tile[bi] = aux[batch_offset.y + col_offsets[j].y + bo.y];
            }
        }
        workgroupBarrier();
        for (var k = 0u; k < 16u; k++) {
            let a = lane / 16u * 16u + k;
            let b = k * 64u + lane % 16u;
            let av = vec4<f32>(left_tile[a], left_tile[a + 256u], left_tile[a + 512u], left_tile[a + 768u]);
            s0 += av * right_tile[b];
            s1 += av * right_tile[b + 16u];
            s2 += av * right_tile[b + 32u];
            s3 += av * right_tile[b + 48u];
        }
        workgroupBarrier();
    }
    for (var j = 0u; j < 4u; j++) {
        if row + j * 16u < m {
            let out = batch_offset.z + row_offsets[j].z;
            if col < n { dst[out + col_offsets[0].z] = s0[j]; }
            if col + 16u < n { dst[out + col_offsets[1].z] = s1[j]; }
            if col + 32u < n { dst[out + col_offsets[2].z] = s2[j]; }
            if col + 48u < n { dst[out + col_offsets[3].z] = s3[j]; }
        }
    }
}
