//! Exercise the public API in fresh processes so environment overrides and
//! the OnceLock are isolated from other tests.

#![cfg(not(target_arch = "wasm32"))]

use std::process::Command;

#[test]
fn feature_flag_environment_overrides() {
    for (overrides, gpu, cloud) in [
        (None, true, false),
        (Some(r#"{"gpu-compositing": false}"#), false, false),
        (Some(r#"{"gpu-compositing": true}"#), true, false),
        (Some(r#"{"gpu-compositing": "false"}"#), true, false),
        (Some("not json"), true, false),
        (Some(r#"{"unknown": true}"#), true, false),
        (Some(r#"{"schist-cloud": true}"#), true, true),
        (Some(r#"{"schist-cloud": false}"#), true, false),
        (Some(r#"{"schist-cloud": "true"}"#), true, false),
        (
            Some(r#"{"schist-cloud": true, "gpu-compositing": false}"#),
            false,
            true,
        ),
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", "feature_flag_child", "--nocapture"])
            .env("SCHIST_TEST_EXPECT_GPU_FLAG", gpu.to_string())
            .env("SCHIST_TEST_EXPECT_CLOUD_FLAG", cloud.to_string())
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
    assert_eq!(
        enabled("schist-cloud"),
        std::env::var("SCHIST_TEST_EXPECT_CLOUD_FLAG").unwrap() == "true"
    );
    assert!(!enabled("unknown"));
    assert!(!enabled("GPU-COMPOSITING"));
    assert!(!enabled(""));
}

#[test]
fn disabled_cloud_ignores_sign_in_callbacks_without_creating_state() {
    let state = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_schist"))
        .arg("schist://ig-callback?state=disabled&code=unused")
        .env("SCHIST_FEATURE_FLAGS", r#"{"schist-cloud":false}"#)
        .env("XDG_STATE_HOME", state.path())
        .env("XDG_CONFIG_HOME", state.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_dir(state.path()).unwrap().count(), 0);
}
