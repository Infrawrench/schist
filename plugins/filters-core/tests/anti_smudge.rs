use schist_plugin_api::{FilterValues, PluginManifest, PluginRegistry};

#[test]
fn anti_smudge_uses_the_embedded_model_without_an_external_install() {
    // Separate test process: deliberately broken external files must not
    // override the bundled model. Pixel/alpha behaviour uses a small arithmetic
    // fixture in the unit tests, avoiding a full-resolution CNN run here.
    let dir = std::env::temp_dir().join(format!("schist-antismudge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("SCHIST_MODEL_DIR", &dir);
    let spec = schist_neural::spec("anti-smudge").unwrap();
    std::fs::write(dir.join("anti-smudge.onnx"), b"old external model").unwrap();
    std::fs::write(dir.join(spec.file), b"broken external archive").unwrap();
    assert!(spec.built_in());
    assert!(schist_neural::installed(spec.id));
    let model = schist_neural::get(spec.id).expect("embedded XZ must unpack and load");
    assert!(std::sync::Arc::ptr_eq(
        &model,
        &schist_neural::get(spec.id).unwrap()
    ));
    assert_eq!(schist_neural::installed_size(spec), Some(spec.bytes as u64));
    assert!(schist_neural::install(spec, b"corrupt download").is_err());
    assert!(schist_neural::install_local(spec, b"external replacement").is_err());
    assert!(schist_neural::uninstall(spec).is_err());

    let mut registry = PluginRegistry::default();
    schist_filters_core::CoreFiltersPlugin.register(&mut registry);
    let filter = registry
        .filters()
        .find(|f| f.id() == "filter.neural.anti_smudge")
        .unwrap();
    let mut values = FilterValues::defaults(&filter.params());
    values.set("strength", 0.0);
    let original = vec![0.8, 0.4, 0.2, 0.5, 0.7, 0.6, 0.3, 0.0];
    let mut pixels = original.clone();
    filter.apply(&mut pixels, 2, 1, &values);
    assert_eq!(pixels, original);
    let info = filter.info().unwrap();
    assert!(info.contains(schist_i18n::t("common.ready")));
    assert!(!info.contains(schist_i18n::t("filter.neural.msg.get_model")));
    schist_neural::forget(spec.id);
    std::env::remove_var("SCHIST_MODEL_DIR");
    std::fs::remove_dir_all(dir).unwrap();
}
