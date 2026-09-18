//! Runtime feature flags. Call [`feature_enabled`] at the feature's entry point.
//!
//! Defaults live here; native builds accept a JSON object in
//! `SCHIST_FEATURE_FLAGS`, and browser builds read the same object from
//! localStorage's `schist.feature_flags` key. Overrides are read once, on
//! the first lookup, so changing them takes a restart (or page reload).

use std::collections::HashMap;
use std::sync::OnceLock;

/// Register each flag here, with its shipping default. Names are exact and
/// case-sensitive; an override cannot enable an unregistered flag.
const DEFAULTS: &[(&str, bool)] = &[("gpu-compositing", true), ("schist-cloud", false)];

static OVERRIDES: OnceLock<HashMap<String, bool>> = OnceLock::new();

/// Whether a named feature is enabled. Unknown names return `false`.
///
/// A configured boolean overrides the shipping default. Missing or malformed
/// configuration leaves the defaults in effect. After the first lookup this
/// only reads memory: callers need no context, client, or async runtime.
///
/// Flags control availability; callers still honour user preferences and
/// platform support before starting a feature.
pub fn feature_enabled(name: &str) -> bool {
    evaluate(
        name,
        OVERRIDES.get_or_init(|| parse_overrides(override_source().as_deref())),
    )
}

fn evaluate(name: &str, overrides: &HashMap<String, bool>) -> bool {
    let Some((_, default)) = DEFAULTS.iter().find(|(key, _)| *key == name) else {
        return false;
    };
    overrides.get(name).copied().unwrap_or(*default)
}

#[cfg(not(target_arch = "wasm32"))]
fn override_source() -> Option<String> {
    std::env::var("SCHIST_FEATURE_FLAGS").ok()
}

#[cfg(target_arch = "wasm32")]
fn override_source() -> Option<String> {
    schist_app_platform::web::local_get("schist.feature_flags")
}

fn parse_overrides(source: Option<&str>) -> HashMap<String, bool> {
    let Some(source) = source else {
        return HashMap::new();
    };
    match serde_json::from_str(source) {
        Ok(overrides) => overrides,
        Err(error) => {
            log::warn!("Ignoring invalid feature flag overrides: {error}");
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_configuration_uses_shipping_defaults() {
        let overrides = parse_overrides(None);
        for &(name, default) in DEFAULTS {
            assert_eq!(evaluate(name, &overrides), default, "{name}");
        }
    }

    #[test]
    fn explicit_false_is_disabled() {
        let overrides = parse_overrides(Some(r#"{"gpu-compositing": false}"#));
        assert!(!evaluate("gpu-compositing", &overrides));
    }

    #[test]
    fn explicit_true_is_enabled() {
        let overrides = parse_overrides(Some(r#"{"gpu-compositing": true}"#));
        assert!(evaluate("gpu-compositing", &overrides));
    }

    #[test]
    fn cloud_and_gpu_compositing_overrides_are_independent() {
        let overrides =
            parse_overrides(Some(r#"{"schist-cloud": true, "gpu-compositing": false}"#));
        assert!(evaluate("schist-cloud", &overrides));
        assert!(!evaluate("gpu-compositing", &overrides));
        let overrides =
            parse_overrides(Some(r#"{"schist-cloud": false, "gpu-compositing": true}"#));
        assert!(!evaluate("schist-cloud", &overrides));
        assert!(evaluate("gpu-compositing", &overrides));
    }

    #[test]
    fn unknown_names_cannot_be_enabled_by_overrides() {
        let overrides = parse_overrides(Some(
            r#"{"unknown": true, "GPU-COMPOSITING": true, "": true, "gpu-compositing": true}"#,
        ));
        for name in ["unknown", "GPU-COMPOSITING", "", " gpu-compositing "] {
            assert!(!evaluate(name, &overrides), "{name:?}");
        }
        assert!(evaluate("gpu-compositing", &overrides));
    }

    #[test]
    fn invalid_configuration_uses_defaults_without_partial_application() {
        for source in [
            "",
            "not json",
            "null",
            "[]",
            r#"{"gpu-compositing": "false"}"#,
            r#"{"gpu-compositing": 0}"#,
            r#"{"gpu-compositing": false, "other": null}"#,
        ] {
            let overrides = parse_overrides(Some(source));
            assert!(overrides.is_empty(), "{source}");
            for &(name, default) in DEFAULTS {
                assert_eq!(evaluate(name, &overrides), default, "{source}: {name}");
            }
        }
    }

    #[test]
    fn registered_names_are_nonempty_and_unique() {
        let mut names = std::collections::HashSet::new();
        for &(name, _) in DEFAULTS {
            assert!(!name.is_empty());
            assert_eq!(name.trim(), name);
            assert!(names.insert(name), "duplicate flag: {name}");
        }
    }
}
