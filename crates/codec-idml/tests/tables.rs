use schist_codec_idml::{container, export, import};
use schist_layout::tables::{CellJustification, RepeatRows};
use schist_layout::{authoring, blank_a4, History, LayoutDocument, Rect, Story};

/// A 2 × 2 table as the public paged-media generator writes it, with
/// InDesign's cell inset spelling on one cell.
fn table(rows: &str) -> String {
    let cell = |name: &str, text: &str, extra: &str| {
        format!(
            r#"<Cell Self="t1i{name}" Name="{name}" RowSpan="1" ColumnSpan="1"{extra}><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/[No paragraph style]"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>{text}</Content></CharacterStyleRange></ParagraphStyleRange></Cell>"#
        )
    };
    format!(
        r#"<Table Self="t1" HeaderRowCount="0" FooterRowCount="0" BodyRowCount="{rows}" ColumnCount="2" AppliedTableStyle="TableStyle/$ID/[No table style]"><Row Self="t1R0" Name="0" SingleRowHeight="28" MinimumHeight="28" AutoGrow="true"/><Row Self="t1R1" Name="1" SingleRowHeight="20" MinimumHeight="3" AutoGrow="false"/><Column Self="t1C0" Name="0" SingleColumnWidth="120"/><Column Self="t1C1" Name="1" SingleColumnWidth="80"/>{}{}{}{}</Table>"#,
        cell(
            "0:0",
            "A1 &amp; é",
            r#" FillColor="Color/Black" FillTint="20""#
        ),
        cell(
            "0:1",
            "A2",
            r#" TextTopInset="6" TextLeftInset="2" VerticalJustification="BottomAlign""#
        ),
        cell(
            "1:0",
            "B1",
            r#" TopEdgeStrokeWeight="2" RightEdgeStrokeWeight="0""#
        ),
        cell("1:1", "B2", ""),
    )
}

/// A native package whose story holds `item` between "Before " and "after".
fn native(item: &str) -> Vec<u8> {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(40.0, 40.0, 400.0, 300.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("Before after", "Body");
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let name = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Stories/"))
        .unwrap()
        .to_owned();
    // The fill names the package's own black swatch.
    let graphic = package.text("Resources/Graphic.xml").unwrap();
    let at = graphic.find(r#"Name="Black""#).unwrap();
    let start = graphic[..at].rfind(r#"Self=""#).unwrap() + 6;
    let black = &graphic[start..start + graphic[start..].find('"').unwrap()];
    let item = item.replace("Color/Black", black);
    let text = package.text(&name).unwrap().to_owned();
    let edited = text.replace(
        "<Content>Before after</Content>",
        &format!("<Content>Before </Content>{item}<Content>after</Content>"),
    );
    package.insert(&name, edited.into_bytes());
    container::write(&package.into_parts())
}

fn typed(doc: &LayoutDocument) -> Option<&schist_layout::tables::Table> {
    doc.stories[0]
        .structures
        .iter()
        .find_map(|s| s.table.as_deref())
}

#[test]
fn tables_are_typed_with_their_cells_stories_and_survive_saves() {
    let mut doc = import::read(&native(&table("2"))).unwrap().document;
    for _ in 0..3 {
        let table = typed(&doc).expect("typed table");
        assert_eq!(table.columns, [120.0, 80.0]);
        assert_eq!(table.rows.len(), 2);
        assert!(table.rows[0].auto_grow && !table.rows[1].auto_grow);
        assert_eq!(table.rows[0].minimum, 28.0);
        let text = |name: (usize, usize)| {
            let cell = table
                .cells
                .iter()
                .find(|c| (c.column, c.row) == name)
                .unwrap();
            doc.stories[cell.story.0 as usize].text()
        };
        assert_eq!(text((0, 0)), "A1 & é");
        assert_eq!(text((1, 1)), "B2");
        let a1 = &table.cells[0];
        assert_eq!(a1.fill.as_ref().map(|f| f.tint), Some(0.2));
        assert_eq!(a1.insets.top, 4.0);
        let a2 = table
            .cells
            .iter()
            .find(|c| (c.column, c.row) == (0, 1))
            .unwrap();
        assert_eq!((a2.insets.top, a2.insets.left), (6.0, 2.0));
        assert_eq!(a2.justification, CellJustification::Bottom);
        let b1 = table
            .cells
            .iter()
            .find(|c| (c.column, c.row) == (1, 0))
            .unwrap();
        assert_eq!(b1.edges[0].weight, 2.0);
        assert_eq!(b1.edges[3].weight, 0.0);
        let flow = schist_layout::compose::compose_story(&doc, schist_layout::StoryId(0));
        assert_eq!(flow.frames[0].unrendered_structures, 0);
        let stories = doc.stories.clone();
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(doc.stories, stories);
    }
}

#[test]
fn tables_whose_counts_disagree_are_reported_and_left_untyped() {
    let imported = import::read(&native(&table("3"))).unwrap();
    assert!(typed(&imported.document).is_none());
    assert!(
        imported.report.skipped.iter().any(|s| s.contains("t1")),
        "{:?}",
        imported.report
    );
    let flow = schist_layout::compose::compose_story(&imported.document, schist_layout::StoryId(0));
    assert_eq!(flow.frames[0].unrendered_structures, 1);
}

#[test]
fn how_a_table_breaks_is_typed_and_unapplied_settings_reported() {
    let xml = table("2")
        .replace(
            r#"<Table Self="t1" "#,
            r#"<Table Self="t1" BreakHeaders="OncePerTextFrame" BreakFooters="OncePerPage" SkipFirstHeader="true" SkipLastFooter="true" "#,
        )
        .replace(
            r#"<Row Self="t1R0" "#,
            r#"<Row Self="t1R0" KeepWithNextRow="true" StartRow="NextFrame" "#,
        );
    let imported = import::read(&native(&xml)).unwrap();
    let mut doc = imported.document;
    for _ in 0..2 {
        let table = typed(&doc).expect("typed table");
        assert_eq!(table.header_repeat, RepeatRows::OncePerFrame);
        assert_eq!(table.footer_repeat, RepeatRows::OncePerPage);
        assert!(table.skip_first_header && table.skip_last_footer);
        assert!(table.rows[0].keep_with_next && !table.rows[1].keep_with_next);
        let stories = doc.stories.clone();
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(doc.stories, stories);
    }
    let skipped = &imported.report.skipped;
    assert!(
        skipped.iter().any(|s| s.contains("StartRow=NextFrame")),
        "{skipped:?}"
    );
    // A value the specification does not define reads as the default.
    let odd = table("2").replace(
        r#"<Table Self="t1" "#,
        r#"<Table Self="t1" BreakHeaders="Sometimes" "#,
    );
    let imported = import::read(&native(&odd)).unwrap();
    assert_eq!(
        typed(&imported.document).unwrap().header_repeat,
        RepeatRows::EveryColumn
    );
    assert!(imported
        .report
        .skipped
        .iter()
        .any(|s| s.contains("BreakHeaders=Sometimes")));
}

/// `native(item)` with `groups` added to its Styles part.
fn styled(item: &str, groups: &str) -> Vec<u8> {
    let mut package = container::read(&native(item)).unwrap();
    let styles = package
        .text("Resources/Styles.xml")
        .unwrap()
        .replace("</idPkg:Styles>", &format!("{groups}</idPkg:Styles>"));
    package.insert("Resources/Styles.xml", styles.into_bytes());
    container::write(&package.into_parts())
}

/// A table style alternating one black row at 30 % with one plain row, and
/// a body region cell style based on another through Properties/BasedOn;
/// a third cell style names its base in an attribute, which InDesign's PDF
/// of the public `styles-cascade` sample ignores.
const STYLES: &str = concat!(
    r#"<RootCellStyleGroup Self="cells">"#,
    r#"<CellStyle Self="CellStyle/$ID/[None]" Name="$ID/[None]"/>"#,
    r#"<CellStyle Self="CellStyle/Base" Name="Base" TextTopInset="9" VerticalJustification="CenterAlign" TopEdgeStrokeWeight="3"/>"#,
    r#"<CellStyle Self="CellStyle/Body" Name="Body"><Properties><BasedOn type="object">CellStyle/Base</BasedOn></Properties></CellStyle>"#,
    r#"<CellStyle Self="CellStyle/Other" Name="Other" TextBottomInset="11"/>"#,
    r#"<CellStyle Self="CellStyle/Loose" Name="Loose" BasedOn="CellStyle/Other"/>"#,
    r#"</RootCellStyleGroup>"#,
    r#"<RootTableStyleGroup Self="tables">"#,
    r#"<TableStyle Self="TableStyle/$ID/[No table style]" Name="$ID/[No table style]"/>"#,
    r#"<TableStyle Self="TableStyle/Stripes" Name="Stripes" StartRowFillCount="1" StartRowFillTint="30" EndRowFillCount="1" BodyRegionCellStyle="CellStyle/Body"/>"#,
    r#"</RootTableStyleGroup>"#,
);

#[test]
fn table_and_cell_styles_type_fills_insets_and_survive_saves() {
    let xml = table("2").replace(
        r#"AppliedTableStyle="TableStyle/$ID/[No table style]""#,
        r#"AppliedTableStyle="TableStyle/Stripes""#,
    );
    // The last cell takes the attribute-linked style directly.
    let xml = xml.replace(
        r#"Name="1:1" RowSpan="1" ColumnSpan="1""#,
        r#"Name="1:1" RowSpan="1" ColumnSpan="1" AppliedCellStyle="CellStyle/Loose""#,
    );
    let mut doc = import::read(&styled(&xml, STYLES)).unwrap().document;
    assert_eq!(doc.retained_table_styles.len(), 2);
    for _ in 0..2 {
        let table = typed(&doc).expect("typed table");
        let stripes = table.row_fills.as_ref().expect("row fills");
        assert_eq!((stripes.first, stripes.next), (1, 1));
        let first = stripes.first_paint.as_ref().unwrap();
        assert_eq!(first.tint, 0.3);
        assert!(stripes.next_paint.is_none());
        assert!(table.column_fills.is_none());
        let cell = |name: (usize, usize)| {
            table
                .cells
                .iter()
                .find(|c| (c.column, c.row) == name)
                .unwrap()
        };
        // The body region's style, through its base, under the cell's own
        // fill.
        let a1 = cell((0, 0));
        assert_eq!(a1.insets.top, 9.0);
        assert_eq!(a1.justification, CellJustification::Center);
        assert_eq!(a1.edges[0].weight, 3.0);
        assert!(a1.own_fill);
        // The cell's own settings win over the styles'.
        let b1 = cell((1, 0));
        assert_eq!(b1.edges[0].weight, 2.0);
        assert!(!b1.own_fill);
        let a2 = cell((0, 1));
        assert_eq!(a2.insets.top, 6.0);
        assert_eq!(a2.justification, CellJustification::Bottom);
        // A style applied to the cell comes before its region's, and one
        // naming its base in an attribute inherits nothing from it.
        let b2 = cell((1, 1));
        assert_eq!(b2.insets.top, 9.0);
        assert_eq!(b2.insets.bottom, 4.0);
        let stories = doc.stories.clone();
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(doc.stories, stories);
    }
}
