//! Print what the reader makes of a package, for working on the reader.
//!
//! `cargo run -p schist-codec-idml --example dump -- <file.idml>`

use schist_codec_idml::import;
use schist_layout::LayoutObject;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: dump <file.idml>");
        std::process::exit(2);
    };
    let bytes = std::fs::read(&path).expect("reading the package");
    let imported = import::read(&bytes).unwrap_or_else(|error| {
        eprintln!("could not read: {error}");
        std::process::exit(1);
    });
    let document = &imported.document;
    println!("pages: {}", document.pages.len());
    for (index, page) in document.pages.iter().enumerate() {
        println!(
            "  {index}: {:?} {}x{} margins {:?} master {:?}",
            page.name, page.width, page.height, page.margins, page.master
        );
    }
    println!("spreads: {}", document.spreads.len());
    for spread in &document.spreads {
        println!("  pages {:?}", spread.pages);
    }
    println!("parents: {}", document.parents.len());
    for parent in &document.parents {
        println!(
            "  {:?} applied_to {:?} objects {}",
            parent.name,
            parent.applied_to,
            parent.objects.len()
        );
    }
    println!("stories: {}", document.stories.len());
    for (index, story) in document.stories.iter().take(3).enumerate() {
        println!(
            "  {index}: {} points, {} ranges",
            story.points.len(),
            story.ranges.len()
        );
        for point in story.points.iter().take(2) {
            if let schist_layout::StoryPoint::Paragraph { text, style } = point {
                println!("    {style:?} {:?}", &text[..text.len().min(60)]);
            }
        }
    }
    println!(
        "inks: {:?}",
        document.inks.iter().map(|i| &i.name).collect::<Vec<_>>()
    );
    println!("objects: {}", document.objects.len());
    for object in document.objects.iter().take(12) {
        let kind = match &object.object {
            LayoutObject::TextFrame { story, columns, .. } => {
                format!("TextFrame story={} columns={columns}", story.0)
            }
            LayoutObject::Shape { .. } => "Shape".to_string(),
            LayoutObject::GraphicFrame { embedded, .. } => format!("Graphic embedded={embedded}"),
            LayoutObject::Note { .. } => "Note".to_string(),
            LayoutObject::Group { .. } => "Group".to_string(),
        };
        println!(
            "  {:?} {kind} page={} bounds={:?} locked={}",
            object.name, object.page, object.bounds, object.locked
        );
    }
    println!("skipped: {}", imported.report.skipped.len());
    for note in imported.report.skipped.iter().take(8) {
        println!("  {note}");
    }
}
