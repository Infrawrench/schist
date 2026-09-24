//! Native Photoshop descriptor regression, reduced to effects only: no artwork,
//! text, paths, or document metadata from the source PSD is needed.
mod common;

use schist_codec_psd::{read_psd, write_psd};
use schist_core::{style::GlowFalloff, BlendMode};

const EFFECTS: &[u8] = include_bytes!("fixtures/photoshop-text-effects.lfx2");

#[test]
fn older_serialized_glows_keep_their_original_falloff() {
    let glow = schist_core::GlowStyle::default();
    let mut stored = serde_json::to_value(glow).unwrap();
    stored.as_object_mut().unwrap().remove("falloff");
    let restored: schist_core::GlowStyle = serde_json::from_value(stored).unwrap();
    assert_eq!(restored, glow);
}

#[test]
fn photoshop_text_effects_import_render_and_survive_saving() {
    let mut psd = common::Psd::rgb8(160, 160);
    let mut layer = common::L::solid("Text effects", (60, 60, 100, 100), [0, 0, 0, 255]);
    layer.extra_blocks.push((*b"lfx2", EFFECTS.to_vec()));
    psd.layers.push(layer);
    let mut doc = read_psd(&psd.build()).unwrap();
    let style = doc.tree.layers[0].style.clone();
    assert!(style.outer_glow.enabled);
    assert!(style.inner_shadow.enabled);
    assert!(style.color_overlay.enabled);
    assert!(
        !style.drop_shadow.enabled,
        "both multi-shadow entries are disabled"
    );
    assert_eq!(style.inner_shadow.settings.blend, BlendMode::Dissolve);
    let glow = style.outer_glow.settings;
    assert_eq!(glow.size, 38.0);
    assert_eq!(glow.spread, 0.21);
    assert_eq!(glow.opacity, 1.0);
    assert_eq!(
        glow.falloff,
        GlowFalloff::Photoshop {
            range: 0.5,
            noise: 0.05
        }
    );
    assert_eq!(style.inner_glow.settings.blend, BlendMode::Screen);
    assert_eq!(style.bevel.settings.shadow_blend, BlendMode::Multiply);

    schist_compositor::restyle_layers(&mut doc.tree.layers, &mut Vec::new());
    let styled = doc.tree.layers[0].styled.as_ref().unwrap();
    let edge = styled.tiles.pixel(55, 80);
    assert!(edge.a > 0.9, "the glow's shoulder disappeared: {edge:?}");
    assert!((edge.r - glow.color.r).abs() < 0.001);
    assert!(
        styled.tiles.pixel(40, 80).a > 0.15,
        "the soft halo disappeared"
    );
    assert!(
        styled.tiles.pixel(20, 80).a < 0.02,
        "the halo extends too far"
    );
    assert_eq!(styled.tiles.pixel(80, 80).to_u8(), [255; 4]);

    let saved = write_psd(&doc).unwrap();
    let reopened = read_psd(&saved).unwrap();
    assert_eq!(reopened.tree.layers[0].style.outer_glow, style.outer_glow);
    assert_eq!(
        reopened.tree.layers[0].style.inner_shadow,
        style.inner_shadow
    );
}
