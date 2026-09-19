use schist_color::{Depth, Rgba};
use schist_core::smart_source::{SmartSource, SourceIdentity};
use schist_core::{Document, Layer, SmartObject, TileCoord};
use schist_document::SharedDocument;

#[test]
fn shared_changes_and_checkpoints_keep_embedded_editable_sources() {
    let mut nested = Document::new("nested", 2, 2, Depth::Eight);
    nested.push_layer(Layer::new_raster("editable layer"));
    let metadata = SmartSource {
        identity: SourceIdentity {
            version: 1,
            id: "family".into(),
            linked_path: None,
            origin: [0, 0],
            linked_stamp: None,
        },
        document: schist_codec_psd::write_psd(&nested).unwrap(),
    };
    let mut doc = Document::new("parent", 4, 4, Depth::Eight);
    let mut layer = Layer::new_raster("instance");
    layer
        .as_raster_mut()
        .unwrap()
        .tiles
        .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, Depth::Eight)
        .set(0, Rgba::WHITE);
    layer.smart = Some(Box::new(SmartObject::wrap(
        layer.as_raster().unwrap().tiles.clone(),
        "source",
    )));
    layer.extras = metadata.blocks(&layer).unwrap();
    let id = doc.push_layer(layer);
    let mut shared = SharedDocument::new(&doc).unwrap();
    let mut peer = SharedDocument::new(&doc).unwrap();
    let mut replacement = metadata.clone();
    replacement.identity.linked_path = Some("/missing/source.psd".into());
    let blocks = replacement.blocks(doc.tree.find(id).unwrap()).unwrap();
    let mut edit = doc.begin_edit("relink");
    edit.set_extras(id, blocks);
    edit.commit();
    peer.apply(&shared.local_changes(&doc).unwrap().unwrap())
        .unwrap();
    let peer_doc = peer.render().unwrap();
    assert_eq!(
        SmartSource::read(&peer_doc.tree.layers[0]).unwrap(),
        Some(replacement.clone())
    );
    let checkpoint = shared.checkpoint().unwrap();
    let mut restored = SharedDocument::unseeded(&doc).unwrap();
    let recovery = restored.restore(&checkpoint, &doc).unwrap();
    assert_eq!(
        SmartSource::read(&recovery.tree.layers[0]).unwrap(),
        Some(replacement)
    );
}
