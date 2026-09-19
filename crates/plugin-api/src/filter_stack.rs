//! Deterministic replay of self-contained filter recipes over their immutable source.
use crate::{
    registry::PluginRegistry, FilterContext, FilterPlugin, FilterValues, NativeFilterBuffer,
};
use anyhow::{ensure, Context, Result};
use schist_color::{Depth, Rgba};
use schist_core::{
    filter_stack::{FilterEffect, FilterStack},
    Selection, TileMap,
};

/// Filters with external documents, live paths, private dialogs, or backdrop
/// dependencies cannot yet be represented as a self-contained stack entry.
pub fn eligible(filter: &dyn FilterPlugin) -> bool {
    !filter.runs_out_of_process()
        && !filter.wants_backdrop()
        && !filter.wants_path()
        && filter.wants_map().is_none()
}

pub fn values(filter: &dyn FilterPlugin, effect: &FilterEffect) -> Result<FilterValues> {
    let specs = filter.params();
    let mut values = FilterValues::defaults(&specs);
    for (key, value) in &effect.values {
        let spec = specs
            .iter()
            .find(|p| p.key == key)
            .context("Filter parameter is no longer available")?;
        ensure!(
            value.is_finite() && *value >= spec.min && *value <= spec.max,
            "Filter parameter outside supported range"
        );
        values.set(spec.key, *value);
    }
    Ok(values)
}

pub fn render(
    registry: &PluginRegistry,
    stack: &FilterStack,
    source: &TileMap,
    depth: Depth,
    profile: Option<Vec<u8>>,
) -> Result<TileMap> {
    stack.validate()?;
    // Disabled and empty stacks restore exact native tiles, including hidden colors.
    if !stack.effects.iter().any(|e| e.enabled) {
        return Ok(source.clone());
    }
    let mut buffer = NativeFilterBuffer::read(source, stack.region, source.mode(), profile);
    for effect in stack.effects.iter().filter(|e| e.enabled) {
        let filter = registry
            .shared_filter(&effect.id)
            .with_context(|| format!("Unavailable filter: {}", effect.id))?;
        ensure!(eligible(filter.as_ref()), "Filter needs external inputs");
        let values = values(filter.as_ref(), effect)?;
        let [r, g, b, a] = effect.foreground;
        let foreground = Rgba::new(r, g, b, a);
        let [r, g, b, a] = effect.background;
        let context = FilterContext {
            foreground,
            background: Rgba::new(r, g, b, a),
            ..Default::default()
        };
        filter.apply_native_with(&mut buffer, &values, &context);
        if let Some(error) = filter.last_error() {
            anyhow::bail!("{}: {error}", effect.id);
        }
    }
    Ok(buffer.write(source, stack.region, depth, &Selection::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FilterParam;
    use schist_core::{filter_stack::FilterEffect, IntRect, TileCoord};
    struct Arithmetic(&'static str, bool);
    impl FilterPlugin for Arithmetic {
        fn id(&self) -> &'static str {
            self.0
        }
        fn name(&self) -> &'static str {
            self.0
        }
        fn params(&self) -> Vec<FilterParam> {
            vec![FilterParam {
                key: "amount",
                label: "amount",
                min: 0.0,
                max: 4.0,
                default: 1.0,
                suffix: "",
                choices: &[],
            }]
        }
        fn apply(&self, pixels: &mut [f32], _: usize, _: usize, v: &FilterValues) {
            for p in pixels.chunks_exact_mut(4) {
                if self.1 {
                    p[0] *= v.get("amount");
                } else {
                    p[0] += v.get("amount");
                }
            }
        }
    }
    fn effect(id: &str, amount: f32) -> FilterEffect {
        FilterEffect {
            id: id.into(),
            enabled: true,
            values: [("amount".into(), amount)].into(),
            foreground: [0.0, 0.0, 0.0, 1.0],
            background: [1.0; 4],
        }
    }
    fn setup() -> (PluginRegistry, TileMap, FilterStack) {
        let mut registry = PluginRegistry::new();
        registry.register_filter(Box::new(Arithmetic("add", false)));
        registry.register_filter(Box::new(Arithmetic("multiply", true)));
        let mut source = TileMap::new();
        source
            .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, Depth::ThirtyTwo)
            .set(0, Rgba::new(0.2, 0.3, 0.4, 1.0));
        let mut stack = FilterStack::new(IntRect::from_size(1, 1));
        stack.effects = vec![effect("add", 0.1), effect("multiply", 2.0)];
        (registry, source, stack)
    }
    #[test]
    fn filter_stack_order_parameters_toggle_and_source_preservation() {
        let (registry, source, mut stack) = setup();
        let run = |stack: &FilterStack| {
            render(&registry, stack, &source, Depth::ThirtyTwo, None)
                .unwrap()
                .pixel(0, 0)
                .r
        };
        assert!((run(&stack) - 0.6).abs() < 0.00001);
        stack.effects.swap(0, 1);
        assert!((run(&stack) - 0.5).abs() < 0.00001);
        stack.effects[1].values.insert("amount".into(), 0.25);
        assert!((run(&stack) - 0.65).abs() < 0.00001);
        stack.effects[0].enabled = false;
        assert!((run(&stack) - 0.45).abs() < 0.00001);
        stack.effects[1].enabled = false;
        assert_eq!(run(&stack), source.pixel(0, 0).r);
        stack.effects.clear();
        assert_eq!(run(&stack), 0.2);
        assert_eq!(source.pixel(0, 0), Rgba::new(0.2, 0.3, 0.4, 1.0));
    }
    #[test]
    fn filter_stack_missing_and_invalid_filter_leave_source_intact() {
        let (registry, source, mut stack) = setup();
        stack.effects[0].id = "missing".into();
        assert!(render(&registry, &stack, &source, Depth::ThirtyTwo, None).is_err());
        stack.effects[0].enabled = false;
        assert!(render(&registry, &stack, &source, Depth::ThirtyTwo, None).is_ok());
        stack.effects[1].values.insert("amount".into(), 99.0);
        assert!(render(&registry, &stack, &source, Depth::ThirtyTwo, None).is_err());
        assert_eq!(source.pixel(0, 0).r, 0.2);
    }
    struct External;
    impl FilterPlugin for External {
        fn id(&self) -> &'static str {
            "external"
        }
        fn name(&self) -> &'static str {
            "external"
        }
        fn wants_map(&self) -> Option<&'static str> {
            Some("map")
        }
        fn apply(&self, _: &mut [f32], _: usize, _: usize, _: &FilterValues) {
            panic!("must not run")
        }
    }
    #[test]
    fn filter_stack_external_inputs_are_rejected() {
        assert!(!eligible(&External));
        let (mut registry, source, mut stack) = setup();
        registry.register_filter(Box::new(External));
        stack.effects = vec![effect("external", 1.0)];
        assert!(render(&registry, &stack, &source, Depth::ThirtyTwo, None).is_err());
    }
}
