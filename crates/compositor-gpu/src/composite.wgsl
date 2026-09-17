// RGB interpreter; prepended with composite_common.wgsl.


@compute @workgroup_size(16, 16, 1)
fn composite(@builtin(global_invocation_id) gid: vec3<u32>) {
    let tile = gid.z;
    if (tile >= globals.n_tiles) {
        return;
    }
    let lx = gid.x;
    let ly = gid.y;
    let px = ly * TILE + lx;
    let orig = tile_origin[tile];
    let x = orig.x + i32(lx);
    let y = orig.y + i32(ly);

    var stack: array<vec4<f32>, MAX_DEPTH>;
    var snap: array<f32, MAX_DEPTH>;
    var sp: u32 = 1u;
    stack[0] = vec4(0.0);

    for (var i = 0u; i < globals.n_ops; i++) {
        let op = ops[i];
        switch op.kind {
            case 0u: {
                // PushLayer: source pixels, mask folded into alpha
                var v = src_texel(op.src_ref, op.src_fmt, tile, px);
                v.a = v.a * mask_value(i, tile, x, y, px);
                stack[sp] = v;
                sp += 1u;
            }
            case 1u: {
                // PushBlank
                stack[sp] = vec4(0.0);
                sp += 1u;
            }
            case 2u: {
                // Blend: pop, blend onto below
                sp -= 1u;
                let s = stack[sp];
                let a = s.a * op.opacity * mask_value(i, tile, x, y, px);
                if (a > 0.0 || op.mode == M_DISSOLVE) {
                    stack[sp - 1u] = blend_px(op.mode, vec4(s.rgb, a), stack[sp - 1u], x, y);
                }
            }
            case 3u: {
                // ClipBlend: confined to the snapshot base alpha
                sp -= 1u;
                let ba = snap[sp - 1u];
                if (ba > 0.0) {
                    let s = stack[sp];
                    let a = s.a * op.opacity * ba;
                    if (a > 0.0 || op.mode == M_DISSOLVE) {
                        stack[sp - 1u] = blend_px(op.mode, vec4(s.rgb, a), stack[sp - 1u], x, y);
                    }
                }
            }
            case 4u: {
                // SnapshotAlpha
                snap[sp - 1u] = stack[sp - 1u].a;
            }
            case 5u: {
                stack[sp - 1u] = adjust_px(i, tile, x, y, px, snap[sp - 1u], stack[sp - 1u]);
            }
            case 6u: {
                // MaskTop: isolated-group mask
                stack[sp - 1u].a *= mask_value(i, tile, x, y, px);
            }
            default: {
            }
        }
    }

    let o = (tile * TILE_PIXELS + px) * 4u;
    let out = stack[0];
    out_f32[o] = out.x;
    out_f32[o + 1u] = out.y;
    out_f32[o + 2u] = out.z;
    out_f32[o + 3u] = out.w;
}
