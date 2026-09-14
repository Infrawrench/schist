//! Exercise the public API in fresh processes so environment overrides and
//! the OnceLock are isolated from other tests.

#![cfg(not(target_arch = "wasm32"))]

use std::process::Command;

#[test]
fn feature_flag_environment_overrides() {
    for (overrides, expected) in [
        (None, true),
        (Some(r#"{"gpu-compositing": false}"#), false),
        (Some(r#"{"gpu-compositing": true}"#), true),
        (Some(r#"{"gpu-compositing": "false"}"#), true),
        (Some("not json"), true),
        (Some(r#"{"unknown": true}"#), true),
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", "feature_flag_child", "--nocapture"])
            .env("SCHIST_TEST_EXPECT_GPU_FLAG", expected.to_string())
            .env_remove("SCHIST_FEATURE_FLAGS");
        if let Some(overrides) = overrides {
            child.env("SCHIST_FEATURE_FLAGS", overrides);
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "overrides {overrides:?}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
fn feature_flag_child() {
    let Ok(expected) = std::env::var("SCHIST_TEST_EXPECT_GPU_FLAG") else {
        return;
    };
    // The main interface is exactly a string-to-boolean function.
    let enabled: fn(&str) -> bool = schist_app::feature_enabled;
    assert_eq!(enabled("gpu-compositing"), expected == "true");
    assert!(!enabled("unknown"));
    assert!(!enabled("GPU-COMPOSITING"));
    assert!(!enabled(""));
}
