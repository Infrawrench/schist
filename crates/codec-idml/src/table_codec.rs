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
    CellEdge, CellJustification, CellPaint, RepeatRows, Table, TableCell, TableRow,
};
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
    let mut stories = Vec::new();
    let mut cells = Vec::new();
    for cell in element.children_named("Cell") {
        let (column, row) = cell.attr("Name")?.split_once(':')?;
        let span = |name| cell.attr(name).map_or(Some(1), |v| v.parse::<usize>().ok());
        let paint = |color: &str, tint: &str, report: &mut Report| {
            let ink = crate::color_codec::resolve(cell, color, colors, report)?;
            Some(CellPaint {
                ink,
                tint: crate::color_codec::tint(cell, tint, report).unwrap_or(1.0),
            })
        };
        let edge = |side: &str, report: &mut Report| {
            let color = format!("{side}EdgeStrokeColor");
            let paint = if cell.attr(&color).is_some() {
                paint(&color, &format!("{side}EdgeStrokeTint"), report)
            } else {
                Some(CellPaint {
                    ink: black.clone(),
                    tint: crate::color_codec::tint(cell, &format!("{side}EdgeStrokeTint"), report)
                        .unwrap_or(1.0),
                })
            };
            CellEdge {
                weight: number(cell, &[&format!("{side}EdgeStrokeWeight")])
                    .unwrap_or(1.0)
                    .max(0.0),
                paint,
            }
        };
        let inset = |name: &str| {
            number(
                cell,
                &[&format!("Text{name}Inset"), &format!("{name}Inset")],
            )
            .unwrap_or(4.0)
        };
        // The cell's paragraphs, with local formatting lowered to styles as a
        // story's are.
        let mut body = cell.clone();
        body.name = "Story".into();
        let body = crate::story_codec::normalize(&body, &mut document.styles, colors, refs, report);
        let story = crate::story_codec::decode(&body);
        cells.push(TableCell {
            column: column.parse().ok()?,
            row: row.parse().ok()?,
            columns: span("ColumnSpan")?,
            rows: span("RowSpan")?,
            story: StoryId((first + stories.len()) as u32),
            fill: paint("FillColor", "FillTint", report),
            insets: Insets {
                top: inset("Top"),
                left: inset("Left"),
                bottom: inset("Bottom"),
                right: inset("Right"),
            },
            justification: match cell.attr("VerticalJustification") {
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
