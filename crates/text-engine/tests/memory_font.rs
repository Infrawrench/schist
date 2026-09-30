use schist_text_engine::{add_font_data, family_names, rasterize, TextSpec};

#[test]
fn an_in_memory_font_resolves_its_family_and_real_cjk_glyphs() {
    let mut spec = TextSpec {
        family: "Noto Sans CJK JP".into(),
        text: "日".into(),
        size: 24.0,
        ..Default::default()
    };
    // Prime the cache before registration: registration must invalidate a
    // previous fallback as well as make the family visible to the chooser.
    let _ = rasterize(&spec);
    add_font_data(include_bytes!("../../../web/fonts/NotoSansJP-Schist.otf").to_vec());
    assert!(family_names().contains(&"Noto Sans CJK JP"));
    let day = rasterize(&spec).unwrap();
    spec.text = "本".into();
    let book = rasterize(&spec).unwrap();
    assert!(!day.is_empty() && !book.is_empty());
    assert_ne!(
        day.coverage, book.coverage,
        "distinct letters cannot both be tofu"
    );
}
