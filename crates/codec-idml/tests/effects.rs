//! An item's blend mode and drop shadow are read and saved; effects Schist
//! does not draw are reported when applied, and settings at InDesign's
//! defaults, as its exports write them on every item, say nothing.
use schist_codec_idml::{container, export, import};
use schist_layout::{
    authoring, blank_a4,
    effects::{BlendMode, DropShadow},
    History, Ink, LayoutDocument, LayoutObject, PlacedObject, Rect,
};

/// The specification's defaults, as InDesign writes them on an item.
const DEFAULTS: &str = r#"<DropShadowSetting Mode="None" BlendMode="Multiply" Opacity="75" XOffset="7" YOffset="7" Size="5" EffectColor="n" Noise="0" Spread="0" UseGlobalLight="false" KnockedOut="true" HonorOtherEffects="false"/><FeatherSetting Mode="None" Width="9" CornerType="Diffusion" Noise="0" ChokeAmount="0"/><InnerShadowSetting Applied="false" EffectColor="n" BlendMode="Multiply" Opacity="75"/><OuterGlowSetting Applied="false"/><InnerGlowSetting Applied="false"/><BevelAndEmbossSetting Applied="false"/><SatinSetting Applied="false"/><DirectionalFeatherSetting Applied="false"/><GradientFeatherSetting Applied="false"/>"#;

/// A package holding one black rectangle whose TransparencySetting holds
/// `inner`, and `outer` after it.
fn package(inner: &str, outer: &str) -> Vec<u8> {
    let mut doc = blank_a4();
    authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(100.0, 100.0, 200.0, 100.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::filled("Black"),
    )
    .unwrap();
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let spread = spread_name(&package);
    let text = package.text(&spread).unwrap().replacen(
        r#"<TransparencySetting><BlendingSetting Opacity="100"/></TransparencySetting>"#,
        &format!("<TransparencySetting>{inner}</TransparencySetting>{outer}"),
        1,
    );
    assert!(text.contains(inner), "{text}");
    package.insert(&spread, text.into_bytes());
    container::write(&package.into_parts())
}

fn spread_name(package: &container::Package) -> String {
    package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Spreads/"))
        .unwrap()
        .to_owned()
}

fn shape(doc: &LayoutDocument) -> &PlacedObject {
    doc.objects
        .iter()
        .find(|o| matches!(o.object, LayoutObject::Shape { .. }))
        .unwrap()
}

fn effects_reported(bytes: &[u8]) -> Vec<String> {
    import::read(bytes)
        .unwrap()
        .report
        .skipped
        .into_iter()
        .filter(|s| s.contains("Effect not applied"))
        .collect()
}

#[test]
fn blend_modes_and_drop_shadows_are_read_and_saved() {
    let bytes = package(
        &format!(
            r#"<BlendingSetting Opacity="80" BlendMode="Multiply"/>{}"#,
            DEFAULTS.replace(
                r#"<DropShadowSetting Mode="None" BlendMode="Multiply" Opacity="75" XOffset="7" YOffset="7" Size="5""#,
                r#"<DropShadowSetting Mode="Drop" BlendMode="Multiply" Opacity="60" XOffset="6" YOffset="-3" Size="12""#,
            )
        ),
        "",
    );
    assert!(
        effects_reported(&bytes).is_empty(),
        "{:?}",
        effects_reported(&bytes)
    );
    let mut doc = import::read(&bytes).unwrap().document;
    let expected = DropShadow {
        opacity: 0.6,
        x_offset: 6.0,
        y_offset: -3.0,
        size: 12.0,
        ..DropShadow::default()
    };
    for _ in 0..2 {
        let object = shape(&doc);
        assert_eq!(object.appearance.blend_mode, Some(BlendMode::Multiply));
        assert_eq!(object.appearance.drop_shadow.as_ref(), Some(&expected));
        assert!((object.transparency - 0.8).abs() < 1e-6);
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
    // A coloured shadow keeps its colour, saved as a swatch.
    let colored = package(
        r#"<DropShadowSetting Mode="Drop" EffectColor="Color/Shade"/>"#,
        "",
    );
    let graphic = |bytes: &[u8]| {
        container::read(bytes)
            .unwrap()
            .text("Resources/Graphic.xml")
            .unwrap()
            .to_owned()
    };
    let with_swatch = {
        let mut package = container::read(&colored).unwrap();
        let text = graphic(&colored).replace(
            "</idPkg:Graphic>",
            r#"<Color Self="Color/Shade" Model="Process" Space="CMYK" ColorValue="0 0 100 20" Name="Shade"/></idPkg:Graphic>"#,
        );
        package.insert("Resources/Graphic.xml", text.into_bytes());
        container::write(&package.into_parts())
    };
    let doc = import::read(&with_swatch).unwrap().document;
    let shadow = shape(&doc).appearance.drop_shadow.clone().unwrap();
    assert_eq!(shadow.ink(), Ink::cmyk("Shade", [0.0, 0.0, 1.0, 0.2]));
    assert!(graphic(&export::write(&doc).bytes).contains(r#"Name="Shade""#));
}

#[test]
fn defaults_say_nothing() {
    let bytes = package(
        &format!(r#"<BlendingSetting Opacity="100" BlendMode="Normal"/>{DEFAULTS}"#),
        r#"<FillTransparencySetting><BlendingSetting Opacity="100" BlendMode="Normal"/></FillTransparencySetting>"#,
    );
    assert!(effects_reported(&bytes).is_empty());
    let doc = import::read(&bytes).unwrap().document;
    assert_eq!(shape(&doc).appearance.blend_mode, None);
    assert_eq!(shape(&doc).appearance.drop_shadow, None);
}

#[test]
fn effects_schist_does_not_draw_are_reported() {
    for (inner, outer, effect) in [
        (r#"<BlendingSetting BlendMode="Hue"/>"#, "", "Hue"),
        (
            r#"<InnerShadowSetting Applied="true"/>"#,
            "",
            "InnerShadowSetting",
        ),
        (r#"<FeatherSetting Mode="Standard"/>"#, "", "FeatherSetting"),
        (
            r#"<DropShadowSetting Mode="Drop" Noise="10"/>"#,
            "",
            "Noise",
        ),
        (
            "",
            r#"<FillTransparencySetting><BlendingSetting Opacity="50"/></FillTransparencySetting>"#,
            "FillTransparencySetting",
        ),
    ] {
        let reported = effects_reported(&package(inner, outer));
        assert!(
            reported.iter().any(|s| s.contains(effect)),
            "{effect}: {reported:?}"
        );
    }
}
