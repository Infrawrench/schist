//! Inspect native run coordinates without printing document text.
use schist_codec_affinity::{
    graph::{self, Value},
    Archive,
};
fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).unwrap();
        let archive = Archive::parse(&bytes).unwrap();
        let graph =
            graph::parse(&archive.extract(archive.head("doc.dat").unwrap()).unwrap()).unwrap();
        if let Some(style) = graph
            .nodes
            .iter()
            .find(|n| n.type_tag() == graph::tag(b"LSty"))
        {
            eprintln!("{path}: first line style {:?}", style.field(b"Data"));
        }
        for block in &graph.nodes {
            let Some(glyphs) = graph.child(block, b"Glyp") else {
                continue;
            };
            let Some(Value::Str(text)) = glyphs.field(b"Utf8") else {
                continue;
            };
            let Some(attrs) = graph.child(block, b"GAtt") else {
                continue;
            };
            let runs: Vec<_> = graph
                .children(attrs, b"Runs")
                .iter()
                .filter_map(|r| match r.field(b"Indx") {
                    Some(Value::I32(v)) => Some(*v),
                    _ => None,
                })
                .collect();
            if !text.is_ascii() {
                println!(
                    "{path}: bytes={} scalars={} utf16={} ends={runs:?}",
                    text.len(),
                    text.chars().count(),
                    text.encode_utf16().count()
                );
            }
        }
    }
}
