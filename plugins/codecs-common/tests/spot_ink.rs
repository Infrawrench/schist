use schist_color::Depth;
use schist_core::{Document, InkChannel};
use schist_plugin_api::CodecPlugin;
#[test]
fn unsupported_layered_exporters_reject_extra_plates_before_losing_data() {
    let mut doc = Document::new("ink", 1, 1, Depth::Eight);
    doc.ink_channels
        .push(InkChannel::spot("Ink".into(), [0.0; 3]));
    let codecs: Vec<Box<dyn CodecPlugin>> = vec![
        Box::new(schist_codecs_common::AffinityCodec),
        Box::new(schist_codecs_common::PdnCodec),
        Box::new(schist_codecs_common::XcfCodec),
    ];
    for codec in codecs {
        assert!(codec.export(&doc).is_err(), "{}", codec.id());
    }
    assert!(schist_codecs_common::PsdCodec.export(&doc).is_ok());
}
