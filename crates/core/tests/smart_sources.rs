use schist_core::smart_source::{SmartSource, SourceIdentity, SOURCE_DOCUMENT_KEY};
use schist_core::{Layer, RawBlock};

fn source() -> SmartSource {
    SmartSource {
        identity: SourceIdentity {
            version: 1,
            id: "source-1".into(),
            linked_path: Some("/missing/art.psd".into()),
            origin: [-12, 7],
            linked_stamp: None,
        },
        document: b"8BPSembedded document".to_vec(),
    }
}

#[test]
fn source_block_roundtrip_replaces_only_its_own_payload() {
    let mut layer = Layer::new_raster("placed");
    layer.extras.push(RawBlock {
        key: *b"test",
        data: vec![9],
    });
    let source = source();
    layer.extras = source.blocks(&layer).unwrap();
    layer.extras = source.blocks(&layer).unwrap();
    assert_eq!(layer.extras.len(), 2);
    assert_eq!(SmartSource::read(&layer).unwrap(), Some(source));
    assert_eq!(layer.extras[0].data, vec![9]);
}

#[test]
fn truncated_and_hostile_metadata_are_rejected() {
    let mut layer = Layer::new_raster("corrupt");
    for data in [
        vec![],
        vec![0, 0, 0],
        u32::MAX.to_be_bytes().to_vec(),
        vec![0, 0, 0, 1, b'{'],
    ] {
        layer.extras = vec![RawBlock {
            key: SOURCE_DOCUMENT_KEY,
            data,
        }];
        assert!(SmartSource::read(&layer).is_err());
    }
    let mut source = source();
    source.identity.version = 999;
    layer.extras = source.blocks(&layer).unwrap();
    assert!(SmartSource::read(&layer).is_err());
    source.identity.version = 1;
    source.identity.origin = [i32::MAX, 0];
    layer.extras = source.blocks(&layer).unwrap();
    assert!(SmartSource::read(&layer).is_err());
}

#[test]
fn history_restores_nested_source_and_link_together() {
    use schist_color::Depth;
    use schist_core::Document;
    let mut doc = Document::new("parent", 4, 4, Depth::Eight);
    let id = doc.push_layer(Layer::new_raster("source"));
    let original = source();
    let blocks = original.blocks(doc.tree.find(id).unwrap()).unwrap();
    let mut edit = doc.begin_edit("embed");
    edit.set_extras(id, blocks);
    edit.commit();
    let mut changed = original.clone();
    changed.identity.linked_path = None;
    changed.document.push(1);
    let blocks = changed.blocks(doc.tree.find(id).unwrap()).unwrap();
    let mut edit = doc.begin_edit("replace");
    edit.set_extras(id, blocks);
    edit.commit();
    doc.undo();
    assert_eq!(
        SmartSource::read(doc.tree.find(id).unwrap()).unwrap(),
        Some(original)
    );
    doc.redo();
    assert_eq!(
        SmartSource::read(doc.tree.find(id).unwrap()).unwrap(),
        Some(changed)
    );
}

#[test]
fn rasterize_removes_editable_source_and_undo_restores_it() {
    use schist_color::Depth;
    use schist_core::{Document, SmartObject, TileMap};
    let mut doc = Document::new("parent", 4, 4, Depth::Eight);
    let mut layer = Layer::new_raster("smart");
    layer.smart = Some(Box::new(SmartObject::wrap(TileMap::new(), "source")));
    layer.extras = source().blocks(&layer).unwrap();
    let id = doc.push_layer(layer);
    let mut edit = doc.begin_edit("rasterize");
    edit.set_smart_object(id, None);
    edit.commit();
    assert!(SmartSource::read(doc.tree.find(id).unwrap())
        .unwrap()
        .is_none());
    doc.undo();
    assert!(doc.tree.find(id).unwrap().smart.is_some());
    assert_eq!(
        SmartSource::read(doc.tree.find(id).unwrap()).unwrap(),
        Some(source())
    );
}
