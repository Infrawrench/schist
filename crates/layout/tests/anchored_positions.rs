//! Items above the line and at custom positions, against InDesign's PDF of
//! the public paged-media `anchored` sample (see `docs/idml-format.md`): a
//! 460 × 720 pt frame at (67.638, 80) on an A4 page, 12 pt text under Auto
//! leading, and a 60 × 36 pt frame stroked 0.5 pt anchored after
//! "reprehenderit " in the second paragraph. The sample's text is Open Sans;
//! these tests set IBM Plex Sans (1000 units, ascender 1025, cap height 698,
//! x-height 516), so numbers that depend on the font use its metrics.
use schist_layout::anchored::{
    AnchorPoint, AnchoredItem, AnchoredPosition, HorizontalAlignment, HorizontalReference,
    Placement, VerticalAlignment, VerticalReference,
};
use schist_layout::{
    anchored, authoring, authoring::ShapeKind, blank_a4, compose::compose_story, geometry::Insets,
    styles::Leading, ComposedLine, History, Ink, LayoutDocument, ObjectId, Page, ParagraphStyle,
    Point, Rect, Spread, Story, StoryId, StoryStructure,
};

const FRAME: Rect = Rect::new(67.638, 80.0, 460.0, 720.0);
const HOST: &str = "Duis aute irure dolor in reprehenderit ";
const SIZE: f32 = 12.0;
const CAP: f32 = 0.698 * SIZE;
const X_HEIGHT: f32 = 0.516 * SIZE;
const ASCENT: f32 = 1.025 * SIZE;

struct Setup {
    position: AnchoredPosition,
    y_offset: f32,
    placement: Placement,
    leading: Leading,
    align: Option<schist_layout::styles::Align>,
    /// Page index of the frame on a two-page facing spread, or None for a
    /// single page.
    facing: Option<usize>,
    margins: Insets,
    wrap: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            position: AnchoredPosition::Inline,
            y_offset: 0.0,
            placement: Placement::default(),
            leading: Leading::Auto,
            align: None,
            facing: None,
            margins: Insets::ZERO,
            wrap: false,
        }
    }
}

struct Composed {
    doc: LayoutDocument,
    frame: ObjectId,
    lines: Vec<ComposedLine>,
    owner: usize,
    /// The item's stroked extent in page space.
    item: Rect,
}

impl Composed {
    fn baseline(&self) -> f32 {
        self.lines[self.owner].baseline
    }
    fn previous(&self) -> f32 {
        self.lines[self.owner - 1].baseline
    }
}

fn compose(setup: Setup) -> Composed {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = blank_a4();
    if let Some(page) = setup.facing {
        doc.pages.push(Page::a4());
        doc.facing_pages = true;
        doc.spreads = vec![Spread {
            pages: vec![0, 1],
            binding_location: Some(1),
            gutter: 0.0,
            origin: Point::ZERO,
        }];
        assert!(page < 2);
    }
    for page in &mut doc.pages {
        page.margins = setup.margins;
    }
    doc.inks.push(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]));
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Lead".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(SIZE),
        leading: Some(setup.leading),
        align: setup.align,
        ..Default::default()
    });
    let page = setup.facing.unwrap_or(0);
    let mut scratch = doc.clone();
    let shape = authoring::shape(
        &mut scratch,
        &mut History::default(),
        page,
        Rect::new(0.0, 0.0, 60.0, 36.0),
        ShapeKind::Rectangle,
        authoring::Paint {
            fill: Some("Cyan".into()),
            stroke: Some("Cyan".into()),
            stroke_width: 0.5,
        },
    )
    .unwrap();
    let mut object = scratch.objects.into_iter().find(|o| o.id == shape).unwrap();
    if setup.wrap {
        object.appearance.text_wrap = Some(schist_layout::text_wrap::TextWrap {
            mode: schist_layout::text_wrap::WrapMode::BoundingBox,
            offsets: Insets::uniform(3.0),
            ..Default::default()
        });
    }
    let id = authoring::text_frame(&mut doc, &mut History::default(), page, FRAME).unwrap();
    let mut story = Story::new();
    story.push_paragraph(
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor \
         incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
         exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.",
        "Lead",
    );
    let (start, _) = story.push_paragraph(
        format!("{HOST}in voluptate velit esse cillum dolore eu fugiat nulla pariatur."),
        "Lead",
    );
    story.structures.push(StoryStructure {
        at: Some(start + HOST.len()),
        kind: "Rectangle".into(),
        payload: "<Rectangle />".into(),
        control: None,
        footnote: None,
        anchored: Some(Box::new(AnchoredItem {
            position: setup.position,
            y_offset: setup.y_offset,
            placement: setup.placement,
            object,
            members: Vec::new(),
        })),
    });
    story.push_paragraph(
        "Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt \
         mollit anim id est laborum.",
        "Lead",
    );
    doc.stories[id.story.0 as usize] = story;
    let lines: Vec<_> = compose_story(&doc, StoryId(0)).lines().cloned().collect();
    let owner = lines
        .iter()
        .position(|l| l.projected.as_ref().is_some_and(|p| !p.anchored.is_empty()))
        .expect("the anchor's line");
    assert!(owner > 0 && owner + 1 < lines.len());
    let frame = doc.object(id.object).unwrap().clone();
    let [placed] = &anchored::placements(&doc, &doc.stories[0], &frame, &lines)[..] else {
        panic!("one placed item")
    };
    let b = placed.visual_bounds();
    let item = Rect::new(b.x - 0.25, b.y - 0.25, b.width + 0.5, b.height + 0.5);
    Composed {
        doc,
        frame: id.object,
        lines,
        owner,
        item,
    }
}

fn near(a: f32, b: f32, what: &str) {
    assert!((a - b).abs() < 0.01, "{what}: {a} vs {b}");
}

fn custom(placement: Placement, y_offset: f32) -> Setup {
    Setup {
        position: AnchoredPosition::Anchored,
        y_offset,
        placement,
        ..Default::default()
    }
}

/// The text frame's right edge, line-relative vertically: TopRightAnchor,
/// TextFrame and RightAlign, as the sample's line-variant pages.
fn line_relative(vertical: VerticalReference) -> Placement {
    Placement {
        anchor_point: AnchorPoint::TopRight,
        horizontal_reference: HorizontalReference::TextFrame,
        horizontal_alignment: HorizontalAlignment::Right,
        vertical_reference: vertical,
        vertical_alignment: VerticalAlignment::Top,
        ..Default::default()
    }
}

/// Native page 2: the anchor line steps the item's height plus its leading
/// (50.9 pt), and with Space After 0 the item's stroked bottom sits half the
/// em plus half the cap height above the baseline (10.2832 pt in Open Sans).
#[test]
fn an_item_above_the_line_takes_its_own_room_above_the_anchor_line() {
    let above = |placement: Placement, y_offset: f32, leading: Leading| {
        compose(Setup {
            position: AnchoredPosition::AboveLine,
            y_offset,
            placement,
            leading,
            ..Default::default()
        })
    };
    let centered = Placement {
        horizontal_alignment: HorizontalAlignment::Center,
        ..Default::default()
    };
    let c = above(centered, 0.0, Leading::Auto);
    near(c.baseline() - c.previous(), 14.4 + 36.5, "anchor line step");
    near(
        c.lines[c.owner + 1].baseline - c.baseline(),
        14.4,
        "next line step",
    );
    near(c.item.bottom(), c.baseline() - (SIZE + CAP) / 2.0, "bottom");
    near(c.item.height, 36.5, "height");
    near(
        c.item.x + c.item.width / 2.0,
        FRAME.x + FRAME.width / 2.0,
        "center",
    );
    // Space above and below add to the step; the item keeps its place
    // relative to the line before.
    let spaced = above(
        Placement {
            space_above: 6.0,
            ..centered
        },
        4.0,
        Leading::Auto,
    );
    near(
        spaced.baseline() - spaced.previous(),
        14.4 + 6.0 + 36.5 + 4.0,
        "spaced step",
    );
    near(
        spaced.item.y - spaced.previous(),
        c.item.y - c.previous() + 6.0,
        "spaced top",
    );
    // Fixed leading keeps its own step under the item.
    let fixed = above(centered, 0.0, Leading::Points(20.0));
    near(
        fixed.baseline() - fixed.previous(),
        20.0 + 36.5,
        "fixed step",
    );
    // Left and right align with the column.
    for (alignment, edge) in [
        (HorizontalAlignment::Left, FRAME.x),
        (HorizontalAlignment::Right, FRAME.right()),
    ] {
        let a = above(
            Placement {
                horizontal_alignment: alignment,
                ..Default::default()
            },
            0.0,
            Leading::Auto,
        );
        let x = if alignment == HorizontalAlignment::Left {
            a.item.x
        } else {
            a.item.right()
        };
        near(x, edge, "aligned edge");
    }
}

#[test]
fn text_alignment_follows_the_paragraph_and_spine_relative_items_mirror() {
    let text = compose(Setup {
        position: AnchoredPosition::AboveLine,
        placement: Placement {
            horizontal_alignment: HorizontalAlignment::Text,
            ..Default::default()
        },
        align: Some(schist_layout::styles::Align::Right),
        ..Default::default()
    });
    near(text.item.right(), FRAME.right(), "text-aligned right");
    // On the left page of a facing spread, left becomes right.
    for (page, edge) in [(0, FRAME.right()), (1, FRAME.x)] {
        let c = compose(Setup {
            position: AnchoredPosition::AboveLine,
            placement: Placement {
                horizontal_alignment: HorizontalAlignment::Left,
                spine_relative: true,
                ..Default::default()
            },
            facing: Some(page),
            ..Default::default()
        });
        assert!(c.doc.page_is_left(0) && !c.doc.page_is_left(1));
        let x = if page == 0 { c.item.right() } else { c.item.x };
        near(x, edge, "spine-relative edge");
    }
}

/// Native pages 4, 8, 10 and 11: the item's anchor point on the frame's
/// top right, the anchor line's baseline, its top of leading (the line
/// before's baseline), and the page margins' bottom right.
#[test]
fn custom_positions_match_the_native_sample() {
    let top_right = Placement {
        vertical_reference: VerticalReference::TextFrame,
        ..line_relative(VerticalReference::TextFrame)
    };
    let c = compose(custom(top_right, 0.0));
    near(c.item.right(), FRAME.right(), "frame right");
    near(c.item.y, FRAME.y, "frame top");
    // The item takes no room: the anchor line keeps the text's step.
    near(c.baseline() - c.previous(), 14.4, "step");
    let c = compose(custom(line_relative(VerticalReference::LineBaseline), 0.0));
    near(c.item.y, c.baseline(), "baseline");
    near(c.item.right(), FRAME.right(), "baseline right");
    let c = compose(custom(line_relative(VerticalReference::TopOfLeading), 0.0));
    near(c.item.y, c.previous(), "top of leading");
    let margins = Insets {
        top: 36.0,
        right: 60.0,
        bottom: 48.0,
        left: 54.0,
    };
    let c = compose(Setup {
        margins,
        ..custom(
            Placement {
                anchor_point: AnchorPoint::BottomRight,
                horizontal_reference: HorizontalReference::PageMargins,
                horizontal_alignment: HorizontalAlignment::Right,
                vertical_reference: VerticalReference::PageMargins,
                vertical_alignment: VerticalAlignment::Bottom,
                ..Default::default()
            },
            0.0,
        )
    });
    let page = &c.doc.pages[0];
    near(c.item.right(), page.width - 60.0, "margin right");
    near(c.item.bottom(), page.height - 48.0, "margin bottom");
    // The line's own metrics.
    for (reference, rise) in [
        (VerticalReference::CapHeight, CAP),
        (VerticalReference::LineXHeight, X_HEIGHT),
        (VerticalReference::LineAscent, ASCENT),
    ] {
        let c = compose(custom(line_relative(reference), 0.0));
        near(c.item.y, c.baseline() - rise, &format!("{reference:?}"));
    }
}

/// Native page 3: AnchorLocation with LeftAlign and offsets of 24 and 12
/// puts the item's top left 24 pt left of the anchor and 12 pt below its
/// baseline. The anchor itself takes no width.
#[test]
fn offsets_from_the_anchor_location_follow_the_native_sample() {
    let pen = compose(Setup::default()).item.x;
    let at_anchor = Placement {
        anchor_point: AnchorPoint::TopLeft,
        horizontal_reference: HorizontalReference::AnchorLocation,
        horizontal_alignment: HorizontalAlignment::Left,
        vertical_reference: VerticalReference::LineBaseline,
        vertical_alignment: VerticalAlignment::Top,
        x_offset: 24.0,
        ..Default::default()
    };
    let c = compose(custom(at_anchor, 12.0));
    near(c.item.x, pen - 24.0, "left");
    near(c.item.y, c.baseline() + 12.0, "top");
    // Right-aligned, the offset moves right (a Schist reading).
    let c = compose(custom(
        Placement {
            horizontal_alignment: HorizontalAlignment::Right,
            ..at_anchor
        },
        0.0,
    ));
    near(c.item.x, pen + 24.0, "right-aligned left");
}

#[test]
fn pinned_items_stay_within_their_frame() {
    let low = |pin_position| {
        compose(custom(
            Placement {
                pin_position,
                ..line_relative(VerticalReference::LineBaseline)
            },
            900.0,
        ))
    };
    near(low(true).item.bottom(), FRAME.bottom(), "pinned");
    let free = low(false);
    near(free.item.y, free.baseline() + 900.0, "unpinned");
}

#[test]
fn unapplied_wrap_is_reported_and_inline_side_offsets_are_not() {
    let wrapped = |position| {
        let c = compose(Setup {
            position,
            wrap: true,
            ..Default::default()
        });
        let flow = compose_story(&c.doc, StoryId(0));
        let frame = flow.frames.iter().find(|f| f.object == c.frame).unwrap();
        assert_eq!(frame.unrendered_structures, 0);
        frame.wrap.ignored
    };
    assert!(!wrapped(AnchoredPosition::Inline));
    assert!(!wrapped(AnchoredPosition::AboveLine));
    assert!(wrapped(AnchoredPosition::Anchored));
}
