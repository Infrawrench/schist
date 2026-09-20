use schist_color::Depth;
use schist_core::{Document, Filter, InkChannel, IntRect};

#[test]
fn plates_follow_crop_canvas_size_and_image_size_in_the_same_undo() {
    let mut doc = Document::new("plates", 3, 1, Depth::ThirtyTwo);
    let mut channel = InkChannel::spot("Ink".into(), [0.0; 3]);
    channel.pixels.set(1, 0, 1.0);
    doc.ink_channels.push(channel);
    schist_tools_transform::crop_to(&mut doc, IntRect::from_xywh(1, 0, 2, 1));
    assert_eq!(doc.width, 2);
    assert_eq!(doc.ink_channels[0].pixels.value(0, 0), 1.0);
    doc.undo();
    assert_eq!(doc.width, 3);
    assert_eq!(doc.ink_channels[0].pixels.value(1, 0), 1.0);
    schist_tools_transform::resize_canvas(&mut doc, 5, 1, (0.5, 0.0));
    assert_eq!(doc.ink_channels[0].pixels.value(2, 0), 1.0);
    doc.undo();
    schist_tools_transform::resize_image(&mut doc, 6, 1, Filter::Bilinear);
    assert_eq!(doc.ink_channels[0].pixels.value(2, 0), 0.75);
    assert_eq!(doc.ink_channels[0].pixels.value(3, 0), 0.75);
    doc.undo();
    assert_eq!(doc.ink_channels[0].pixels.value(1, 0), 1.0);
    assert_eq!(doc.ink_channels[0].pixels.value(2, 0), 0.0);
}
