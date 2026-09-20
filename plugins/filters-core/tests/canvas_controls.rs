use schist_filters_core::{
    blurgallery::{FieldBlur, IrisBlur, PathBlur, SpinBlur, TiltShift},
    other::RadialBlur,
    render::{LensFlare, LightingEffects},
};
use schist_plugin_api::{FilterPlugin, FilterValues};

fn filters() -> Vec<Box<dyn FilterPlugin>> {
    vec![
        Box::new(FieldBlur),
        Box::new(IrisBlur),
        Box::new(PathBlur),
        Box::new(SpinBlur),
        Box::new(TiltShift),
        Box::new(RadialBlur),
        Box::new(LensFlare),
        Box::new(LightingEffects),
    ]
}

#[test]
fn canvas_controls_round_trip_to_numeric_parameters_and_match_pixels() {
    for filter in filters() {
        let specs = filter.params();
        let mut values = FilterValues::defaults(&specs);
        let controls = filter.canvas_controls(&values);
        assert!(!controls.is_empty(), "{}", filter.id());
        for control in &controls {
            for (index, handle) in control
                .geometry(&values, (37.0, 23.0))
                .handles
                .iter()
                .enumerate()
            {
                assert!(specs.iter().any(|p| p.key == handle.key), "{}", filter.id());
                let original = values.clone();
                control.move_handle(index, handle.point, (37.0, 23.0), &mut values, &specs);
                for (key, before) in original.0 {
                    assert!(
                        (values.get(key) - before).abs() < 0.0001,
                        "{} {key}",
                        filter.id()
                    );
                }
            }
        }
        let before = values.clone();
        assert!(
            controls[0].move_handle(0, (10.0, 5.0), (37.0, 23.0), &mut values, &specs),
            "{}",
            filter.id()
        );
        let mut numeric = before.clone();
        for (key, value) in &values.0 {
            numeric.set(key, *value);
        }
        let input: Vec<f32> = (0..37 * 23)
            .flat_map(|i| {
                [
                    if i % 3 == 0 { 0.9 } else { 0.1 },
                    (i % 37) as f32 / 37.0,
                    (i / 37) as f32 / 23.0,
                    1.0,
                ]
            })
            .collect();
        let mut old = input.clone();
        let mut dragged = input.clone();
        let mut typed = input;
        filter.apply(&mut old, 37, 23, &before);
        filter.apply(&mut dragged, 37, 23, &values);
        filter.apply(&mut typed, 37, 23, &numeric);
        assert_eq!(dragged, typed, "{}", filter.id());
        assert_ne!(old, dragged, "{} must affect output", filter.id());
    }
}

#[test]
fn lighting_handles_match_the_active_light_type() {
    let filter = LightingEffects;
    let mut v = FilterValues::defaults(&filter.params());
    for kind in 0..3 {
        v.set("type", kind as f32);
        let g = filter.canvas_controls(&v)[0].geometry(&v, (200.0, 100.0));
        let keys: Vec<_> = g.handles.iter().map(|h| h.key).collect();
        assert_eq!(
            keys,
            if kind == 2 {
                vec!["angle"]
            } else {
                vec!["x", "spread"]
            }
        );
    }
}
