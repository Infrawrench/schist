//! Tables inside stories. The retained XML stays the saved form; tables are
//! typed from it for composition, each cell's text becoming a story of the
//! document. Attribute names follow the public specification and the cell
//! inset spelling InDesign writes (`TextTopInset` …); absent cell insets are
//! the 4 pt InDesign's PDF of the public paged-media `tables` sample shows,
//! absent edges InDesign's default 1 pt black. How the table breaks follows
//! the specification's BreakHeaders, BreakFooters, SkipFirstHeader,
//! SkipLastFooter and each row's KeepWithNextRow; a row StartRow other than
//! Anywhere, and values the specification does not define, are reported and
//! read as their defaults.
use crate::{import::Report, xml};
use schist_layout::tables::{
    Alternation, CellEdge, CellJustification, CellPaint, RepeatRows, Table, TableCell, TableRow,
};

/// The root styles every table and cell style is ultimately based on.
const NO_TABLE_STYLE: &str = "TableStyle/$ID/[No table style]";
const NO_CELL_STYLE: &str = "CellStyle/$ID/[None]";

/// Style `start` and those it is based on, nearest first, ending with the
/// root style `root` when the package defines it. Only the specification's
/// Properties/BasedOn element links styles: InDesign's PDF of the public
/// paged-media `styles-cascade` sample leaves a table plain whose cell and
/// table styles name their bases in a BasedOn attribute instead.
fn chain<'a>(
    styles: &'a std::collections::BTreeMap<String, xml::Element>,
    start: Option<&str>,
    root: &str,
) -> Vec<&'a xml::Element> {
    let mut out: Vec<&xml::Element> = Vec::new();
    let mut next = start.filter(|s| !s.is_empty() && *s != "n");
    while let Some(id) = next {
        let Some(style) = styles.get(id) else {
            break;
        };
        if out.iter().any(|seen| std::ptr::eq(*seen, style)) {
            break;
        }
        out.push(style);
        next = style
            .child("Properties")
            .and_then(|p| p.child("BasedOn"))
            .map(xml::Element::trimmed)
            .filter(|v| !v.is_empty() && *v != "n");
    }
    if let Some(root) = styles.get(root) {
        if !out.iter().any(|seen| std::ptr::eq(*seen, root)) {
            out.push(root);
        }
    }
    out
}

/// The table and cell style groups of a Styles part, as written.
pub(crate) fn style_groups(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for group in ["RootCellStyleGroup", "RootTableStyleGroup"] {
        let open = format!("<{group}");
        let mut from = 0;
        while let Some(at) = text[from..].find(&open).map(|i| from + i) {
            let after = at + open.len();
            // Only the element itself, not a longer name sharing the prefix.
            if !text[after..].starts_with([' ', '>', '/', '\t', '\r', '\n']) {
                from = after;
                continue;
            }
            let Some(tag_end) = text[after..].find('>').map(|i| after + i) else {
                break;
            };
            let end = if text[..tag_end].ends_with('/') {
                tag_end + 1
            } else {
                let close = format!("</{group}>");
                match text[tag_end..].find(&close) {
                    Some(i) => tag_end + i + close.len(),
                    None => break,
                }
            };
            out.push(text[at..end].to_owned());
            from = end;
        }
    }
    out
}

/// The first of `owners` that states `name`.
fn owner<'a>(owners: &[&'a xml::Element], name: &str) -> Option<&'a xml::Element> {
    owners.iter().copied().find(|e| e.attr(name).is_some())
}
use schist_layout::{Insets, LayoutDocument, StoryId};

pub(crate) fn read(
    document: &mut LayoutDocument,
    colors: &crate::color_codec::Colors,
    refs: &crate::style_codec::References,
    report: &mut Report,
) {
    for story in 0..document.stories.len() {
        for index in 0..document.stories[story].structures.len() {
            let structure = &document.stories[story].structures[index];
            if structure.table.is_some() || structure.kind != "Table" {
                continue;
            }
            let Ok(element) = xml::parse(&structure.payload) else {
                continue;
            };
            let first = document.stories.len();
            match table(&element, document, first, colors, refs, report) {
                Some((table, stories)) => {
                    document.stories.extend(stories);
                    document.stories[story].structures[index].table = Some(Box::new(table));
                }
                None => report.skip(schist_i18n::tf!(
                    "design.idml_table_invalid",
                    name = element.attr("Self").unwrap_or_default()
                )),
            }
        }
    }
}

fn number(element: &xml::Element, names: &[&str]) -> Option<f32> {
    names
        .iter()
        .find_map(|name| element.attr(name))
        .and_then(xml::parse_number)
        .filter(|v| v.is_finite())
}

/// The table and its cells' stories, numbered from `first`; None when its
/// counts, sizes or cells do not agree.
fn table(
    element: &xml::Element,
    document: &mut LayoutDocument,
    first: usize,
    colors: &crate::color_codec::Colors,
    refs: &crate::style_codec::References,
    report: &mut Report,
) -> Option<(Table, Vec<schist_layout::Story>)> {
    let count = |name| {
        element
            .attr(name)
            .map_or(Some(0), |v| v.parse::<usize>().ok())
    };
    let (header, body, footer) = (
        count("HeaderRowCount")?,
        count("BodyRowCount")?,
        count("FooterRowCount")?,
    );
    let name = element.attr("Self").unwrap_or_default();
    // A setting composition does not apply, reported once by attribute.
    let mut unapplied = std::collections::BTreeSet::new();
    let flag = |owner: &xml::Element,
                attribute: &str,
                unapplied: &mut std::collections::BTreeSet<String>| {
        owner.attr(attribute).is_some_and(|value| {
            xml::parse_boolean(value).unwrap_or_else(|| {
                unapplied.insert(format!("{attribute}={value}"));
                false
            })
        })
    };
    let mut rows: Vec<TableRow> = Vec::new();
    for row in element.children_named("Row") {
        let height = number(row, &["SingleRowHeight"]).unwrap_or(20.0);
        if let Some(start) = row.attr("StartRow").filter(|s| *s != "Anywhere") {
            unapplied.insert(format!("StartRow={start}"));
        }
        rows.push(TableRow {
            height,
            minimum: number(row, &["MinimumHeight"]).unwrap_or(3.0),
            maximum: number(row, &["MaximumHeight"]),
            auto_grow: match row.attr("AutoGrow") {
                None => true,
                Some(value) => xml::parse_boolean(value)?,
            },
            keep_with_next: flag(row, "KeepWithNextRow", &mut unapplied),
        });
    }
    let repeat = |attribute: &str, unapplied: &mut std::collections::BTreeSet<String>| match element
        .attr(attribute)
    {
        None | Some("InAllTextColumns") => RepeatRows::EveryColumn,
        Some("OncePerTextFrame") => RepeatRows::OncePerFrame,
        Some("OncePerPage") => RepeatRows::OncePerPage,
        Some(other) => {
            unapplied.insert(format!("{attribute}={other}"));
            RepeatRows::EveryColumn
        }
    };
    let header_repeat = repeat("BreakHeaders", &mut unapplied);
    let footer_repeat = repeat("BreakFooters", &mut unapplied);
    let skip_first_header = flag(element, "SkipFirstHeader", &mut unapplied);
    let skip_last_footer = flag(element, "SkipLastFooter", &mut unapplied);
    let columns: Vec<f32> = element
        .children_named("Column")
        .map(|column| number(column, &["SingleColumnWidth"]))
        .collect::<Option<_>>()?;
    if rows.len() != header + body + footer
        || element
            .attr("ColumnCount")
            .is_some_and(|c| c.parse::<usize>().ok() != Some(columns.len()))
    {
        return None;
    }
    let black = colors
        .get("Color/Black")
        .cloned()
        .unwrap_or_else(schist_layout::Ink::black);
    // The table's own settings, then its style's and those it is based on.
    let mut table_owners = vec![element];
    table_owners.extend(chain(
        &refs.table_styles,
        element.attr("AppliedTableStyle"),
        NO_TABLE_STYLE,
    ));
    let table_attr = |name: &str| owner(&table_owners, name).and_then(|e| e.attr(name));
    // Alternating fills, with the defaults InDesign's own exports write for
    // [No table style]: no pattern, a first fill of black at 20 %, a next
    // fill of none. InDesign's PDF of the public `tables` sample tints its
    // style's 20 % cyan first fill to 4 %.
    let alternation = |axis: &str, report: &mut Report| -> Option<Alternation> {
        let count = |name: String| {
            table_attr(&name)
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
        };
        let (first, next) = (
            count(format!("Start{axis}FillCount")),
            count(format!("End{axis}FillCount")),
        );
        if first + next == 0 {
            return None;
        }
        let paint = |which: &str, tint: f32, report: &mut Report| {
            let color = format!("{which}{axis}FillColor");
            let ink = match owner(&table_owners, &color) {
                Some(e) => crate::color_codec::resolve(e, &color, colors, report)?,
                None if which == "Start" => black.clone(),
                None => return None,
            };
            let key = format!("{which}{axis}FillTint");
            let tint = owner(&table_owners, &key)
                .and_then(|e| crate::color_codec::tint(e, &key, report))
                .unwrap_or(tint);
            Some(CellPaint { ink, tint })
        };
        Some(Alternation {
            first,
            first_paint: paint("Start", 0.2, report),
            next,
            next_paint: paint("End", 1.0, report),
            skip_first: count(format!("SkipFirstAlternatingFill{axis}s")),
            skip_last: count(format!("SkipLastAlternatingFill{axis}s")),
        })
    };
    // ColumnFillsPriority false hides column fills, true row fills, as the
    // specification says: the public `tables` PDF draws none for a style
    // that gives only column fills.
    let columns_first = table_attr("ColumnFillsPriority")
        .and_then(xml::parse_boolean)
        .unwrap_or(false);
    let row_fills = if columns_first {
        None
    } else {
        alternation("Row", report)
    };
    let column_fills = if columns_first {
        alternation("Column", report)
    } else {
        None
    };
    // The cell style a table region gives a cell: the header's or footer's
    // when not the body's, then the left or right column's, then the body's.
    let column_count = columns.len();
    let region = |row: usize, column: usize, span: usize| {
        let own = |region: &str| {
            let same = table_attr(&format!("{region}RegionSameAsBodyRegion"))
                .and_then(xml::parse_boolean)
                .unwrap_or(true);
            (!same)
                .then(|| table_attr(&format!("{region}RegionCellStyle")))
                .flatten()
        };
        (if row < header {
            own("Header")
        } else if row >= header + body {
            own("Footer")
        } else {
            None
        })
        .or_else(|| (column == 0).then(|| own("LeftColumn")).flatten())
        .or_else(|| {
            (column + span == column_count)
                .then(|| own("RightColumn"))
                .flatten()
        })
        .or_else(|| table_attr("BodyRegionCellStyle"))
    };
    let mut stories = Vec::new();
    let mut cells = Vec::new();
    for cell in element.children_named("Cell") {
        let (column, row) = cell.attr("Name")?.split_once(':')?;
        let span = |name| cell.attr(name).map_or(Some(1), |v| v.parse::<usize>().ok());
        let (column, row): (usize, usize) = (column.parse().ok()?, row.parse().ok()?);
        let (columns_spanned, rows_spanned) = (span("ColumnSpan")?, span("RowSpan")?);
        // The cell's own settings, then its cell style's, then the cell style
        // its table region gives it; the table's own cell settings last, for
        // insets and justification only.
        let mut owners = vec![cell];
        owners.extend(chain(
            &refs.cell_styles,
            cell.attr("AppliedCellStyle")
                .filter(|s| *s != NO_CELL_STYLE),
            NO_CELL_STYLE,
        ));
        owners.extend(chain(
            &refs.cell_styles,
            region(row, column, columns_spanned).filter(|s| *s != NO_CELL_STYLE),
            NO_CELL_STYLE,
        ));
        let styled = owners.len();
        owners.push(element);
        let styled = &owners[..styled];
        let paint = |color: &str, tint: &str, report: &mut Report| {
            let ink = crate::color_codec::resolve(owner(styled, color)?, color, colors, report)?;
            Some(CellPaint {
                ink,
                tint: owner(styled, tint)
                    .and_then(|e| crate::color_codec::tint(e, tint, report))
                    .unwrap_or(1.0),
            })
        };
        let edge = |side: &str, report: &mut Report| {
            let color = format!("{side}EdgeStrokeColor");
            let tint = format!("{side}EdgeStrokeTint");
            let paint = if owner(styled, &color).is_some() {
                paint(&color, &tint, report)
            } else {
                Some(CellPaint {
                    ink: black.clone(),
                    tint: owner(styled, &tint)
                        .and_then(|e| crate::color_codec::tint(e, &tint, report))
                        .unwrap_or(1.0),
                })
            };
            let weight = format!("{side}EdgeStrokeWeight");
            CellEdge {
                weight: styled
                    .iter()
                    .find_map(|e| number(e, &[&weight]))
                    .unwrap_or(1.0)
                    .max(0.0),
                paint,
            }
        };
        let inset = |name: &str| {
            owners
                .iter()
                .find_map(|e| number(e, &[&format!("Text{name}Inset"), &format!("{name}Inset")]))
                .unwrap_or(4.0)
        };
        let justification = owners.iter().find_map(|e| e.attr("VerticalJustification"));
        // The cell's paragraphs, with local formatting lowered to styles as a
        // story's are.
        let mut body = cell.clone();
        body.name = "Story".into();
        let body = crate::story_codec::normalize(&body, &mut document.styles, colors, refs, report);
        let story = crate::story_codec::decode(&body);
        cells.push(TableCell {
            column,
            row,
            columns: columns_spanned,
            rows: rows_spanned,
            story: StoryId((first + stories.len()) as u32),
            fill: paint("FillColor", "FillTint", report),
            own_fill: owner(styled, "FillColor").is_some(),
            insets: Insets {
                top: inset("Top"),
                left: inset("Left"),
                bottom: inset("Bottom"),
                right: inset("Right"),
            },
            justification: match justification {
                None | Some("TopAlign") | Some("JustifyAlign") => CellJustification::Top,
                Some("CenterAlign") => CellJustification::Center,
                Some("BottomAlign") => CellJustification::Bottom,
                Some(_) => return None,
            },
            edges: [
                edge("Top", report),
                edge("Left", report),
                edge("Bottom", report),
                edge("Right", report),
            ],
        });
        stories.push(story);
    }
    let table = Table {
        header_rows: header,
        footer_rows: footer,
        rows,
        columns,
        cells,
        header_repeat,
        footer_repeat,
        skip_first_header,
        skip_last_footer,
        row_fills,
        column_fills,
    };
    if !table.valid() {
        return None;
    }
    for setting in unapplied {
        report.skip(schist_i18n::tf!(
            "design.idml_table_setting",
            name = name,
            setting = setting
        ));
    }
    Some((table, stories))
}
