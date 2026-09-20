use schist_compositor::viewport::{
    periodic_regions, render_viewport_cpu, render_viewport_periodic_cpu, ViewportParams,
};
use schist_core::{IntRect, TILE_PIXELS, TILE_SIZE};
use std::sync::Arc;

fn params() -> ViewportParams {
    ViewportParams {
        width: 6,
        height: 6,
        origin: (2.0, 2.0),
        zoom: 1.0,
        scale_factor: 1.0,
        rotation: 0.0,
        canvas: IntRect::from_size(2, 2),
        grid_origin: (0, 0),
        grid_cols: 1,
        grid_rows: 1,
        surround: 0x343434,
    }
}
fn grid() -> Vec<Option<Arc<Vec<u8>>>> {
    let mut tile = vec![0; TILE_PIXELS * 4];
    for (x, y, c) in [
        (0, 0, [255, 0, 0, 255]),
        (1, 0, [0, 255, 0, 255]),
        (0, 1, [0, 0, 255, 255]),
        (1, 1, [255, 255, 255, 255]),
    ] {
        let at = (y * TILE_SIZE as usize + x) * 4;
        tile[at..at + 4].copy_from_slice(&c);
    }
    vec![Some(Arc::new(tile))]
}
fn pixel(bytes: &[u8], width: usize, x: usize, y: usize) -> &[u8] {
    &bytes[(y * width + x) * 4..(y * width + x + 1) * 4]
}

#[test]
fn repeat_samples_negative_coordinates_and_does_not_enlarge_the_source() {
    let p = params();
    let tiles = grid();
    let repeated = render_viewport_periodic_cpu(&p, &tiles);
    for y in 0..6 {
        for x in 0..6 {
            assert_eq!(
                pixel(&repeated, 6, x, y),
                pixel(&repeated, 6, 2 + x % 2, 2 + y % 2)
            );
        }
    }
    assert_eq!(pixel(&repeated, 6, 0, 0), [0, 0, 255, 255]);
    let normal = render_viewport_cpu(&p, &tiles);
    assert_eq!(pixel(&normal, 6, 0, 0), [0x34, 0x34, 0x34, 255]);
    assert_eq!(p.canvas, IntRect::from_size(2, 2));
}

#[test]
fn fractional_zoom_interpolates_across_repeat_edges_without_a_transparent_seam() {
    let mut p = params();
    p.width = 12;
    p.height = 12;
    p.zoom = 1.5;
    p.origin = (0.0, 0.0);
    let out = render_viewport_periodic_cpu(&p, &grid());
    for y in 0..9 {
        for x in 0..9 {
            assert_eq!(pixel(&out, 12, x, y), pixel(&out, 12, x + 3, y + 3));
        }
    }
}

#[test]
fn rotated_repeated_view_still_uses_document_coordinates() {
    let mut p = params();
    p.rotation = std::f32::consts::FRAC_PI_2;
    let out = render_viewport_periodic_cpu(&p, &grid());
    for y in 0..4 {
        for x in 0..4 {
            for (a, b) in pixel(&out, 6, x, y)
                .iter()
                .zip(pixel(&out, 6, x + 2, y + 2))
            {
                assert!((*a as i32 - *b as i32).abs() <= 1);
            }
        }
    }
}

#[test]
fn periodic_visible_regions_split_edges_without_loading_unseen_tiles() {
    let canvas = IntRect::from_size(1000, 800);
    let regions = periodic_regions(IntRect::new(-20, -10, 30, 40), canvas);
    assert_eq!(regions.len(), 4);
    assert_eq!(
        regions.iter().map(|r| r.width() * r.height()).sum::<i32>(),
        2500
    );
    assert!(regions.iter().all(|r| r.intersect(&canvas) == *r));
    assert_eq!(
        periodic_regions(IntRect::new(-1000, -1000, 1000, 1000), canvas),
        vec![canvas]
    );
    assert!(periodic_regions(IntRect::EMPTY, canvas).is_empty());
}
