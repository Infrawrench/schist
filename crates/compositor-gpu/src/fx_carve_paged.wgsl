// Paged variant of fx_carve.wgsl. Image planes live in texture arrays;
// cumulative costs need only two rows, and all seams remain on the device.

// Content-Aware Scale: the whole seam-carving loop, on the device.
//
// Unlike the other fx kernels this is not one sweep but hundreds, each
// depending on the last: find the lowest-energy top-to-bottom path, remove
// it, recompute, repeat. Coming back to the CPU between seams would cost
// more in round trips than the work is worth, so every stage lives here
// and only the finished image is read back.
//
// The awkward stage is the cumulative-cost scan, which is sequential down
// the rows. One dispatch per row would be tens of thousands of dispatches
// for one command, so `dp_tile` does TILE_ROWS rows at a time: a workgroup
// loads a span wider than it owns and lets the valid region shrink by one
// column per row, which is exactly how far the ±1 dependency spreads. The
// arithmetic mirrors `schist_fx`'s reference stage for stage.

// state[] slots. Everything that changes between dispatches lives here, so
// one bind group serves the whole run.
const S_WIDTH: u32 = 0u;      // current width, shrinking or growing
const S_HEIGHT: u32 = 1u;
const S_STRIDE: u32 = 2u;     // row stride of every buffer: the widest width
const S_MODE: u32 = 3u;       // 0 = carve, 1 = grow
const S_BEST: u32 = 5u;       // column the seam ends on
const S_SEAM: u32 = 8u;       // seam[y] follows, one column per row

const TILE_ROWS: u32 = 64u;
const WG: u32 = 256u;
// Columns per thread. One keeps the workgroup count -- and so the
// parallelism -- up; carrying several columns each would cut the barrier
// traffic but leaves too few workgroups to fill a device.
const PER_THREAD: u32 = 1u;
const SPAN: u32 = WG * PER_THREAD;
// What a workgroup owns: the span it loads, less the column the cone gives
// up at each end for every row after the first.
const TILE_COLS: u32 = SPAN - 2u * (TILE_ROWS - 1u);

const BIG: f32 = 3.4028235e38;

@group(0) @binding(0) var<storage, read_write> state: array<i32>;
@group(0) @binding(1) var px_in: texture_2d_array<f32>;
@group(0) @binding(2) var px_out: texture_storage_2d_array<rgba32float, write>;
@group(0) @binding(3) var prot_in: texture_2d_array<f32>;
@group(0) @binding(4) var prot_out: texture_storage_2d_array<r32float, write>;
@group(0) @binding(5) var energy_out: texture_storage_2d_array<r32float, write>;
@group(0) @binding(6) var<storage, read_write> cost: array<f32>;
@group(0) @binding(7) var from_out: texture_storage_2d_array<r32sint, write>;
@group(0) @binding(9) var energy_in: texture_2d_array<f32>;
@group(0) @binding(10) var from_in: texture_2d_array<i32>;
// Which band of rows this dispatch scans. A uniform with a dynamic offset
// rather than another counter in `state`: bumping a counter would need its
// own dispatch between every tile, and there are tens of thousands of them
// in one command.
struct Tile {
    top: u32,
    // Three scalars rather than a vec3, whose 16-byte alignment would push
    // the struct to 32 and past the binding size declared for it.
    _p0: u32,
    _p1: u32,
    _p2: u32,
}
@group(0) @binding(8) var<uniform> tile: Tile;

fn width() -> u32 {
    return u32(state[S_WIDTH]);
}

fn height() -> u32 {
    return u32(state[S_HEIGHT]);
}

fn stride() -> u32 {
    return u32(state[S_STRIDE]);
}

// State slots 6 and 7 hold the texture page dimensions. Logical image
// rows can cross pages; neighbours are always addressed in image space.
fn texel(x: u32, y: u32) -> vec3<i32> {
    let i = y * stride() + x;
    let tw = u32(state[6]);
    let th = u32(state[7]);
    return vec3<i32>(i32(i % tw), i32((i / tw) % th), i32(i / (tw * th)));
}
fn pixel(x: u32, y: u32) -> vec4<f32> {
    let p = texel(x, y);
    return textureLoad(px_in, p.xy, p.z, 0);
}
fn protection(x: u32, y: u32) -> f32 {
    let p = texel(x, y);
    return textureLoad(prot_in, p.xy, p.z, 0).x;
}
fn energy(x: u32, y: u32) -> f32 {
    let p = texel(x, y);
    return textureLoad(energy_in, p.xy, p.z, 0).x;
}
fn lum(x: u32, y: u32) -> f32 {
    let p = pixel(x, y);
    return 0.299 * p.x + 0.587 * p.y + 0.114 * p.z;
}

// Gradient magnitude plus protection, with edge pixels clamping to
// themselves exactly as the reference's saturating_sub/min do.
@compute @workgroup_size(16, 16, 1)
fn energy_pass(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = width();
    let h = height();
    if (gid.x >= w || gid.y >= h) {
        return;
    }
    let x = gid.x;
    let y = gid.y;
    var xl = 0u;
    if (x > 0u) {
        xl = x - 1u;
    }
    var yu = 0u;
    if (y > 0u) {
        yu = y - 1u;
    }
    let l = lum(xl, y);
    let r = lum(min(x + 1u, w - 1u), y);
    let u = lum(x, yu);
    let d = lum(x, min(y + 1u, h - 1u));
    // Fully transparent pixels are free to remove.
    let alpha = pixel(x, y).a;
    let p = texel(x, y);
    textureStore(energy_out, p.xy, p.z,
        vec4((abs(r - l) + abs(d - u)) * alpha + protection(x, y), 0.0, 0.0, 0.0));
}

// Row 0 of the scan is just the energy, which is where the reference's
// `cost = energy.clone()` starts it.
@compute @workgroup_size(WG, 1, 1)
fn dp_seed(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= width()) {
        return;
    }
    cost[gid.x] = energy(gid.x, 0u);
    let p = texel(gid.x, 0u);
    textureStore(from_out, p.xy, p.z, vec4<i32>(0));
}

// Two rows of the scan, alternating: reading one while writing the other
// needs a single barrier a row instead of two.
var<workgroup> ring: array<array<f32, SPAN>, 2>;

// TILE_ROWS rows of the cumulative-cost scan.
//
// A workgroup owns TILE_COLS columns but loads TILE_ROWS-1 extra either
// side. Only columns in [r, SPAN-1-r] hold a correct value at row r, and
// the owned range sits inside that at every row, so what gets written is
// always what a whole-row scan would have written.
@compute @workgroup_size(WG, 1, 1)
fn dp_tile(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let w = width();
    let h = height();
    let s = stride();
    let own0 = wid.x * TILE_COLS;
    if (own0 >= w) {
        return;
    }
    let own1 = min(own0 + TILE_COLS, w); // exclusive
    let top = tile.top + 1u;
    // First column of the loaded span, as a signed value: it runs off the
    // left edge for the first workgroup.
    let span0 = i32(own0) - i32(TILE_ROWS - 1u);

    // Seed from the row above, which a previous dispatch finished.
    for (var k = 0u; k < PER_THREAD; k++) {
        let e = lid.x * PER_THREAD + k;
        let c = span0 + i32(e);
        var v = BIG;
        if (c >= 0 && c < i32(w)) {
            v = cost[((tile.top / TILE_ROWS) & 1u) * s + u32(c)];
        }
        ring[0][e] = v;
    }

    for (var r = 0u; r < TILE_ROWS; r++) {
        let y = top + r;
        if (y >= h) {
            break;
        }
        workgroupBarrier();
        let src = r & 1u;
        let dst = 1u - src;
        for (var k = 0u; k < PER_THREAD; k++) {
            let e = lid.x * PER_THREAD + k;
            let c = span0 + i32(e);
            var out = BIG;
            var dir = 0;
            if (c >= 0 && c < i32(w)) {
                // Straight up first, then left, then right, each on a
                // strict less-than: the reference's tie-breaking, which
                // decides which of several equal-cost seams gets carved.
                var best = ring[src][e];
                if (c > 0 && e > 0u && ring[src][e - 1u] < best) {
                    best = ring[src][e - 1u];
                    dir = -1;
                }
                if (c + 1 < i32(w) && e + 1u < SPAN && ring[src][e + 1u] < best) {
                    best = ring[src][e + 1u];
                    dir = 1;
                }
                out = energy(u32(c), y) + best;
            }
            if (c >= i32(own0) && c < i32(own1)) {
                // Only the last row is needed by the next dispatch.
                // Alternate row buffers so neighbouring workgroups never
                // overwrite a seed that another is still reading.
                cost[(1u - ((tile.top / TILE_ROWS) & 1u)) * s + u32(c)] = out;
                let p = texel(u32(c), y);
                textureStore(from_out, p.xy, p.z, vec4<i32>(dir, 0, 0, 0));
            }
            ring[dst][e] = out;
        }
    }
}

var<workgroup> best_val: array<f32, WG>;
var<workgroup> best_col: array<u32, WG>;

// Cheapest end column, then walk the seam back up. The scan is sequential
// in the rows, so it is one thread — h steps, against the millions the
// rest of a seam costs.
@compute @workgroup_size(WG, 1, 1)
fn pick(@builtin(local_invocation_id) lid: vec3<u32>) {
    let w = width();
    let h = height();
    let s = stride();
    let base = (((h - 1u + TILE_ROWS - 1u) / TILE_ROWS) & 1u) * s;
    var bv = BIG;
    var bc = 0u;
    for (var x = lid.x; x < w; x += WG) {
        let v = cost[base + x];
        if (v < bv) {
            bv = v;
            bc = x;
        }
    }
    best_val[lid.x] = bv;
    best_col[lid.x] = bc;
    // Lowest cost wins, and the lowest column breaks the tie — which is
    // what `min_by` does, since it keeps the first of equal elements.
    for (var step = WG / 2u; step > 0u; step >>= 1u) {
        workgroupBarrier();
        if (lid.x < step) {
            let o = lid.x + step;
            if (best_val[o] < best_val[lid.x]
                || (best_val[o] == best_val[lid.x] && best_col[o] < best_col[lid.x])) {
                best_val[lid.x] = best_val[o];
                best_col[lid.x] = best_col[o];
            }
        }
    }
    workgroupBarrier();
    if (lid.x != 0u) {
        return;
    }
    var x = i32(best_col[0]);
    state[S_BEST] = x;
    for (var y = i32(h) - 1; y >= 0; y--) {
        state[S_SEAM + u32(y)] = x;
        let p = texel(u32(x), u32(y));
        let d = textureLoad(from_in, p.xy, p.z, 0).x;
        x = clamp(x + d, 0, i32(w) - 1);
    }
}

// Rebuild the image either side of the seam. Every output pixel names its
// own source, so this is the one stage that is trivially parallel.
@compute @workgroup_size(16, 16, 1)
fn resample(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = width();
    let h = height();
    let s = stride();
    if (gid.y >= h) {
        return;
    }
    let y = gid.y;
    let x = gid.x;
    let cut = u32(state[S_SEAM + y]);
    let p = texel(x, y);
    if (state[S_MODE] == 0) {
        if (x + 1u >= w) { return; }
        var sx = x;
        if (x >= cut) { sx = x + 1u; }
        textureStore(px_out, p.xy, p.z, pixel(sx, y));
        textureStore(prot_out, p.xy, p.z, vec4(protection(sx, y), 0.0, 0.0, 0.0));
        return;
    }
    if (x > w) { return; }
    if (x == cut + 1u) {
        textureStore(px_out, p.xy, p.z, (pixel(cut, y) + pixel(min(cut + 1u, w - 1u), y)) / 2.0);
        textureStore(prot_out, p.xy, p.z, vec4(protection(cut, y) + 200.0, 0.0, 0.0, 0.0));
        return;
    }
    var sx = x;
    if (x > cut) { sx = x - 1u; }
    textureStore(px_out, p.xy, p.z, pixel(sx, y));
    textureStore(prot_out, p.xy, p.z, vec4(protection(sx, y), 0.0, 0.0, 0.0));
}

@compute @workgroup_size(1, 1, 1)
fn advance_seam() {
    if (state[S_MODE] == 0) {
        state[S_WIDTH] = state[S_WIDTH] - 1;
    } else {
        state[S_WIDTH] = state[S_WIDTH] + 1;
    }
}
