//! Exercise warm uploads and copy-on-write invalidation on the real GPU.
use schist_color::{Depth, Rgba};
use schist_compositor::{
    viewport::{render_viewport_cpu, ViewportParams},
    Compositor, CpuCompositor,
};
use schist_compositor_gpu::{plan, BatchOut, GpuContext};
use schist_core::{Document, Layer, LayerMask, TileBuf, TileCoord, TILE_PIXELS};
use std::sync::Arc;

fn render(ctx: &GpuContext, doc: &Document, coords: &[TileCoord]) -> Vec<Vec<u8>> {
    let plan = plan::build(doc).unwrap();
    let Some(BatchOut::Rgba8(gpu)) = ctx.composite_batch(&plan, coords, true) else {
        panic!("GPU composite declined");
    };
    let cpu = CpuCompositor.tiles_rgba8(doc, coords);
    assert_eq!(gpu.len(), cpu.len());
    assert!(gpu.iter().all(|tile| tile.len() == TILE_PIXELS * 4));
    for (gpu, cpu) in gpu.iter().flatten().zip(cpu.iter().flatten()) {
        assert!((*gpu as i32 - *cpu as i32).abs() <= 1);
    }
    gpu
}

#[test]
fn warm_uploads_follow_drag_edits_masks_and_viewport_changes() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("skipping GPU upload cache test: {e}");
            return;
        }
    };
    let mut doc = Document::new("uploads", 512, 256, Depth::Eight);
    let coord = TileCoord { tx: 0, ty: 0 };
    let coords = [coord, TileCoord { tx: 1, ty: 0 }];
    let mut layer = Layer::new_raster("pixels");
    for (coord, depth) in coords.iter().zip([Depth::Eight, Depth::Sixteen]) {
        let mut tile = TileBuf::new(depth);
        for i in 0..TILE_PIXELS {
            tile.set(i, Rgba::new((i % 256) as f32 / 255.0, 0.4, 0.7, 0.8));
        }
        layer
            .as_raster_mut()
            .unwrap()
            .tiles
            .insert(*coord, Arc::new(tile));
    }
    let mut mask = LayerMask::new_revealing();
    mask.bounds = doc.canvas_rect();
    mask.tiles.get_mut_or_insert(coord).fill(180);
    layer.mask = Some(mask);
    doc.push_layer(layer);
    doc.tree.layers[0].render_offset = (1, 1);
    let first = render(&ctx, &doc, &coords);
    let hits = ctx.tile_upload_stats().hits;
    doc.tree.layers[0].render_offset = (7, 9);
    let shifted = render(&ctx, &doc, &coords);
    assert_ne!(first, shifted);
    assert!(
        ctx.tile_upload_stats().hits >= hits + 2,
        "pixels and mask reused while dragging"
    );

    // There are no extra strong owners: the cache's Weak must still force
    // copy-on-write to change the identity when the pixels or mask change.
    doc.tree.layers[0]
        .as_raster_mut()
        .unwrap()
        .tiles
        .get_mut_or_insert(coord, Depth::Eight)
        .set(0, Rgba::new(0.0, 1.0, 0.0, 1.0));
    doc.tree.layers[0]
        .mask
        .as_mut()
        .unwrap()
        .tiles
        .get_mut_or_insert(coord)
        .fill(90);
    let edited = render(&ctx, &doc, &coords);
    assert_ne!(shifted, edited);
    for offset in [(-13, -7), (256, 0), (0, 0)] {
        doc.tree.layers[0].render_offset = offset;
        render(&ctx, &doc, &coords);
    }

    let mut grid: Vec<_> = edited
        .into_iter()
        .map(|tile| Some(Arc::new(tile)))
        .collect();
    let mut params = ViewportParams {
        width: 320,
        height: 240,
        origin: (0.0, 0.0),
        zoom: 1.0,
        scale_factor: 1.0,
        rotation: 0.0,
        canvas: doc.canvas_rect(),
        grid_origin: (0, 0),
        grid_cols: 2,
        grid_rows: 1,
        surround: 0x343434,
    };
    for frame in 0..4 {
        params.origin.0 = -(frame as f32 * 3.0);
        if frame == 2 {
            Arc::make_mut(grid[0].as_mut().unwrap()).fill(255);
        }
        if frame == 3 {
            grid.swap(0, 1);
        }
        let hits = ctx.tile_upload_stats().hits;
        let gpu = ctx
            .render_viewport(&params, &grid)
            .expect("GPU viewport declined");
        assert_eq!(gpu, render_viewport_cpu(&params, &grid));
        if frame == 1 {
            assert_eq!(ctx.tile_upload_stats().hits, hits + 1);
        }
    }
    let stats = ctx.tile_upload_stats();
    assert!(stats.bytes > 0 && stats.bytes <= 64 << 20);
    ctx.clear_tile_uploads();
    assert_eq!(ctx.tile_upload_stats().bytes, 0);
}

#[test]
fn shifted_byte_tiles_fit_without_budgeting_them_as_floats() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("skipping GPU batch-budget test: {e}");
            return;
        }
    };
    let mut doc = Document::new("batch budget", 512, 512, Depth::Eight);
    let mut layer = Layer::new_raster("pixels");
    let mut tile = TileBuf::new(Depth::Eight);
    for i in 0..TILE_PIXELS {
        tile.set(i, Rgba::new(0.2, 0.4, 0.6, 0.8));
    }
    let tile = Arc::new(tile);
    for coord in TileCoord::covering(&doc.canvas_rect()) {
        layer
            .as_raster_mut()
            .unwrap()
            .tiles
            .insert(coord, tile.clone());
    }
    layer.render_offset = (1, 1);
    doc.push_layer(layer);
    // Four byte tiles fit in the same binding as one f32 output tile.
    let limit = ctx.binding_limit();
    ctx.set_binding_limit(TILE_PIXELS * 16);
    let coords = [TileCoord { tx: 1, ty: 1 }];
    render(&ctx, &doc, &coords);

    // A wider neighbour widens the whole source row. It must decline
    // before allocating a binding that no longer fits the forced limit.
    doc.tree.layers[0].as_raster_mut().unwrap().tiles.insert(
        TileCoord { tx: 0, ty: 0 },
        Arc::new(TileBuf::new(Depth::Sixteen)),
    );
    assert!(ctx
        .composite_batch(&plan::build(&doc).unwrap(), &coords, true)
        .is_none());
    ctx.set_binding_limit(limit);
    render(&ctx, &doc, &coords);
}
