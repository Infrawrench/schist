//! Smart albums in the window: the rule editor, the sidebar's
//! description of an album, and the one bit of their evaluation that
//! touches the disk. What an album *is* — the rules and their verdicts
//! — lives in `schist_gallery::smart`; the matches are kept current by
//! the same background pass that fills the query and area buckets
//! (`refresh_smart_buckets`), since a smart album is a bucket with
//! rules on it.

use super::gallery_chrome::pal;
use super::*;
use schist_gallery::culling::{ColourLabel, CullFlag};
use schist_gallery::smart::{Compare, FileKind, Match, PhotoFacts, Rule, RuleGroup, RuleKind};
use schist_gallery_ui::culling::{colour_name, flag_name};
use schist_i18n::{t, tf, tn};

/// The most rules (groups included, at every depth) one album holds. A
/// dropdown or a text field is keyed by a static id, so every rule row
/// has its own fixed set of them.
pub(super) const MAX_RULES: usize = 16;

/// Per row: the kind dropdown, the value dropdown, a second value
/// dropdown (a rating's stars), and two text fields (a date range's
/// ends; every other text rule uses the first).
const KIND_POPUPS: [&str; MAX_RULES] = [
    "smart-kind-0",
    "smart-kind-1",
    "smart-kind-2",
    "smart-kind-3",
    "smart-kind-4",
    "smart-kind-5",
    "smart-kind-6",
    "smart-kind-7",
    "smart-kind-8",
    "smart-kind-9",
    "smart-kind-10",
    "smart-kind-11",
    "smart-kind-12",
    "smart-kind-13",
    "smart-kind-14",
    "smart-kind-15",
];
const VALUE_POPUPS: [&str; MAX_RULES] = [
    "smart-value-0",
    "smart-value-1",
    "smart-value-2",
    "smart-value-3",
    "smart-value-4",
    "smart-value-5",
    "smart-value-6",
    "smart-value-7",
    "smart-value-8",
    "smart-value-9",
    "smart-value-10",
    "smart-value-11",
    "smart-value-12",
    "smart-value-13",
    "smart-value-14",
    "smart-value-15",
];
const STAR_POPUPS: [&str; MAX_RULES] = [
    "smart-stars-0",
    "smart-stars-1",
    "smart-stars-2",
    "smart-stars-3",
    "smart-stars-4",
    "smart-stars-5",
    "smart-stars-6",
    "smart-stars-7",
    "smart-stars-8",
    "smart-stars-9",
    "smart-stars-10",
    "smart-stars-11",
    "smart-stars-12",
    "smart-stars-13",
    "smart-stars-14",
    "smart-stars-15",
];
const TEXT_FIELDS: [&str; MAX_RULES] = [
    "smart-text-0",
    "smart-text-1",
    "smart-text-2",
    "smart-text-3",
    "smart-text-4",
    "smart-text-5",
    "smart-text-6",
    "smart-text-7",
    "smart-text-8",
    "smart-text-9",
    "smart-text-10",
    "smart-text-11",
    "smart-text-12",
    "smart-text-13",
    "smart-text-14",
    "smart-text-15",
];
const TO_FIELDS: [&str; MAX_RULES] = [
    "smart-to-0",
    "smart-to-1",
    "smart-to-2",
    "smart-to-3",
    "smart-to-4",
    "smart-to-5",
    "smart-to-6",
    "smart-to-7",
    "smart-to-8",
    "smart-to-9",
    "smart-to-10",
    "smart-to-11",
    "smart-to-12",
    "smart-to-13",
    "smart-to-14",
    "smart-to-15",
];
/// The album's name field and the top group's match dropdown.
pub(super) const NAME_FIELD: &str = "smart-name";
const TOP_MATCH_POPUP: &str = "smart-match-top";

/// "3 rules", for the bucket header and the AI panel's description.
pub(super) fn rules_summary(rules: &RuleGroup) -> String {
    tn("smart_album.n_rules", rules.len() as u64)
}

/// Mark the photos whose edits have kept earlier versions. One listing
/// of each folder's `.schist/versions` answers for every photo in it;
/// run on the background executor, and only for albums that ask.
pub(super) fn fill_versions(facts: &mut [PhotoFacts]) {
    let mut listed: FxHashMap<PathBuf, std::collections::HashSet<String>> = FxHashMap::default();
    for fact in facts {
        let capture = schist_gallery::variants::capture(&fact.path);
        let (Some(dir), Some(sidecar)) = (
            capture.parent(),
            schist_gallery::backing_psd(&capture)
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
        ) else {
            continue;
        };
        let kept = listed
            .entry(dir.to_path_buf())
            .or_insert_with(|| schist_gallery::smart::versioned_sidecars(dir));
        fact.has_versions = kept.contains(&sidecar);
    }
}

/// The rule at `path` (child indices from the top group down).
fn rule_at_mut<'a>(group: &'a mut RuleGroup, path: &[usize]) -> Option<&'a mut Rule> {
    let (first, rest) = path.split_first()?;
    let rule = group.rules.get_mut(*first)?;
    if rest.is_empty() {
        return Some(rule);
    }
    match rule {
        Rule::Group(inner) => rule_at_mut(inner, rest),
        _ => None,
    }
}

/// Every rule's path in display order — what numbers the rows, and so
/// which static ids each row's controls use.
fn row_paths(group: &RuleGroup) -> Vec<Vec<usize>> {
    fn walk(group: &RuleGroup, prefix: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        for (i, rule) in group.rules.iter().enumerate() {
            prefix.push(i);
            out.push(prefix.clone());
            if let Rule::Group(inner) = rule {
                walk(inner, prefix, out);
            }
            prefix.pop();
        }
    }
    let mut out = Vec::new();
    walk(group, &mut Vec::new(), &mut out);
    out
}

/// Put a typed value where its field belongs: the name, or one of a
/// rule row's text fields. Called by the dialog's field commit.
pub(super) fn commit_field(modal: &mut Modal, id: &str, buffer: String) {
    let Modal::SmartAlbum { name, rules, .. } = modal else {
        return;
    };
    if id == NAME_FIELD {
        *name = buffer;
        return;
    }
    let (row, to) = if let Some(at) = TEXT_FIELDS.iter().position(|f| *f == id) {
        (at, false)
    } else if let Some(at) = TO_FIELDS.iter().position(|f| *f == id) {
        (at, true)
    } else {
        return;
    };
    let Some(path) = row_paths(rules).get(row).cloned() else {
        return;
    };
    match rule_at_mut(rules, &path) {
        Some(Rule::Taken { from, to: end }) => {
            if to {
                *end = buffer.trim().to_string();
            } else {
                *from = buffer.trim().to_string();
            }
        }
        Some(
            Rule::Keyword { text }
            | Rule::Camera { text }
            | Rule::Lens { text }
            | Rule::Place { text },
        ) => *text = buffer,
        Some(Rule::Person { name }) => *name = buffer,
        _ => {}
    }
}

fn kind_name(kind: RuleKind) -> &'static str {
    t(match kind {
        RuleKind::Rating => "smart_album.kind.rating",
        RuleKind::Flag => "smart_album.kind.flag",
        RuleKind::Label => "smart_album.kind.label",
        RuleKind::Keyword => "smart_album.kind.keyword",
        RuleKind::Taken => "smart_album.kind.taken",
        RuleKind::Camera => "smart_album.kind.camera",
        RuleKind::Lens => "smart_album.kind.lens",
        RuleKind::FileType => "smart_album.kind.file_type",
        RuleKind::HasLocation => "smart_album.kind.has_location",
        RuleKind::Place => "smart_album.kind.place",
        RuleKind::Person => "smart_album.kind.person",
        RuleKind::Folder => "smart_album.kind.folder",
        RuleKind::Edited => "smart_album.kind.edited",
        RuleKind::HasVersions => "smart_album.kind.has_versions",
        RuleKind::Group => "smart_album.group",
    })
}

fn match_name(matching: Match) -> &'static str {
    t(match matching {
        Match::All => "smart_album.match.all",
        Match::Any => "smart_album.match.any",
        Match::None => "smart_album.match.none",
    })
}

fn compare_name(compare: Compare) -> &'static str {
    t(match compare {
        Compare::AtLeast => "smart_album.compare.at_least",
        Compare::AtMost => "smart_album.compare.at_most",
        Compare::Exactly => "smart_album.compare.exactly",
    })
}

fn file_name(file: FileKind) -> &'static str {
    t(match file {
        FileKind::Jpeg => "smart_album.file.jpeg",
        FileKind::Png => "smart_album.file.png",
        FileKind::Heif => "smart_album.file.heif",
        FileKind::Avif => "smart_album.file.avif",
        FileKind::JpegXl => "smart_album.file.jpeg_xl",
        FileKind::Webp => "smart_album.file.webp",
        FileKind::Tiff => "smart_album.file.tiff",
        FileKind::Raw => "smart_album.file.raw",
        FileKind::Layered => "smart_album.file.layered",
        FileKind::Video => "smart_album.file.video",
        FileKind::Other => "smart_album.file.other_kind",
    })
}

fn yes_no(yes: bool) -> &'static str {
    if yes {
        t("common.yes")
    } else {
        t("common.no")
    }
}

impl Workspace {
    /// Open the editor on a new, empty smart album.
    pub(crate) fn gallery_new_smart_album(&mut self, cx: &mut Context<Self>) {
        self.library.context = None;
        self.open_modal(
            Modal::SmartAlbum {
                editing: None,
                name: String::new(),
                rules: RuleGroup {
                    matching: Match::All,
                    rules: vec![RuleKind::Rating.default_rule()],
                },
            },
            cx,
        );
        self.focus_field(NAME_FIELD, "");
    }

    /// Reopen the editor on an album: rename it, change its rules.
    pub(super) fn gallery_edit_smart_album(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(bucket) = self.library.buckets.get(index) else {
            return;
        };
        let modal = Modal::SmartAlbum {
            editing: Some(index),
            name: bucket.name.clone(),
            rules: bucket.filter.clone().unwrap_or_default(),
        };
        self.library.context = None;
        self.open_modal(modal, cx);
    }

    /// Change the editor's rules in place.
    fn edit_smart_rules(&mut self, f: impl FnOnce(&mut RuleGroup)) {
        // Whatever was being typed belongs to the rows as they are now,
        // before they move.
        self.commit_focused_field();
        self.update_modal(|m| {
            if let Modal::SmartAlbum { rules, .. } = m {
                f(rules);
            }
        });
    }

    fn save_smart_album(&mut self, cx: &mut Context<Self>) {
        self.commit_focused_field();
        let Some(Modal::SmartAlbum {
            editing,
            name,
            rules,
        }) = self.modal.clone()
        else {
            return;
        };
        let name = match name.trim() {
            "" => match editing.and_then(|i| self.library.buckets.get(i)) {
                Some(bucket) => bucket.name.clone(),
                None => tf!(
                    "smart_album.default_name",
                    n = self.library.buckets.iter().filter(|b| b.is_album()).count() + 1
                ),
            },
            typed => typed.to_string(),
        };
        let index = match editing {
            Some(index) => index,
            None => self.library.add_bucket(name.clone()),
        };
        if let Some(bucket) = self.library.buckets.get_mut(index) {
            bucket.name = name;
        }
        self.library.set_bucket_filter(index, Some(rules));
        self.close_modal(cx);
        // Show what it holds: that is what was just made.
        if editing.is_none() {
            self.library.bucket_filter = Some(index);
            self.library.folder_filter = None;
            self.library.person_filter = None;
            self.library.selected.clear();
        }
        cx.notify();
    }
}

/// A text field in a rule row.
fn text_field(
    id: &'static str,
    value: String,
    placeholder: String,
    width: f32,
    ws: &Workspace,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    super::library_view::bucket_field(id, value, placeholder, ws, cx).w(px(width))
}

/// One of a row's dropdowns.
#[allow(clippy::too_many_arguments)]
fn choice<T: Clone + PartialEq + 'static>(
    ws: &Workspace,
    popup: &'static str,
    current: T,
    label: impl Into<SharedString>,
    width: f32,
    options: Vec<(SharedString, T)>,
    on_select: impl Fn(&mut Workspace, T) + Clone + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    crate::ui::dropdown(
        &ws.dropdown,
        crate::ui::Dropdown {
            popup: Popup::Field(popup),
            is_open: ws.open_popup == Some(Popup::Field(popup)),
            current,
            label: label.into(),
            width,
            options,
        },
        move |ws, value, _cx| on_select(ws, value),
        cx,
    )
}

/// The value controls for one rule.
fn rule_values(
    ws: &Workspace,
    row: usize,
    path: Vec<usize>,
    rule: &Rule,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let row_div = || div().flex().flex_row().items_center().gap_1();
    match rule.clone() {
        Rule::Rating { compare, stars } => {
            let p = path.clone();
            let p2 = path;
            row_div()
                .child(choice(
                    ws,
                    VALUE_POPUPS[row],
                    compare,
                    compare_name(compare),
                    110.0,
                    Compare::ALL
                        .iter()
                        .map(|c| (SharedString::from(compare_name(*c)), *c))
                        .collect(),
                    move |ws, value| {
                        ws.edit_smart_rules(|rules| {
                            if let Some(Rule::Rating { compare, .. }) = rule_at_mut(rules, &p) {
                                *compare = value;
                            }
                        })
                    },
                    cx,
                ))
                .child(choice(
                    ws,
                    STAR_POPUPS[row],
                    stars,
                    tn("smart_album.stars", stars as u64),
                    100.0,
                    (0..=5u8)
                        .map(|n| (SharedString::from(tn("smart_album.stars", n as u64)), n))
                        .collect(),
                    move |ws, value| {
                        ws.edit_smart_rules(|rules| {
                            if let Some(Rule::Rating { stars, .. }) = rule_at_mut(rules, &p2) {
                                *stars = value;
                            }
                        })
                    },
                    cx,
                ))
                .into_any_element()
        }
        Rule::Flag { flag } => choice(
            ws,
            VALUE_POPUPS[row],
            flag,
            flag_name(flag),
            160.0,
            [CullFlag::Pick, CullFlag::Reject, CullFlag::None]
                .iter()
                .map(|f| (SharedString::from(flag_name(*f)), *f))
                .collect(),
            move |ws, value| {
                ws.edit_smart_rules(|rules| {
                    if let Some(Rule::Flag { flag }) = rule_at_mut(rules, &path) {
                        *flag = value;
                    }
                })
            },
            cx,
        )
        .into_any_element(),
        Rule::Label { label } => choice(
            ws,
            VALUE_POPUPS[row],
            label,
            colour_name(label),
            160.0,
            [
                ColourLabel::Red,
                ColourLabel::Yellow,
                ColourLabel::Green,
                ColourLabel::Blue,
                ColourLabel::Magenta,
                ColourLabel::None,
            ]
            .iter()
            .map(|l| (SharedString::from(colour_name(*l)), *l))
            .collect(),
            move |ws, value| {
                ws.edit_smart_rules(|rules| {
                    if let Some(Rule::Label { label }) = rule_at_mut(rules, &path) {
                        *label = value;
                    }
                })
            },
            cx,
        )
        .into_any_element(),
        Rule::Keyword { text }
        | Rule::Camera { text }
        | Rule::Lens { text }
        | Rule::Place { text } => text_field(
            TEXT_FIELDS[row],
            text,
            t("smart_album.text_placeholder").to_string(),
            220.0,
            ws,
            cx,
        )
        .into_any_element(),
        Rule::Taken { from, to } => row_div()
            .child(text_field(
                TEXT_FIELDS[row],
                from,
                t("smart_album.date_from").to_string(),
                120.0,
                ws,
                cx,
            ))
            .child(div().child("–"))
            .child(text_field(
                TO_FIELDS[row],
                to,
                t("smart_album.date_to").to_string(),
                120.0,
                ws,
                cx,
            ))
            .into_any_element(),
        Rule::FileType { file } => choice(
            ws,
            VALUE_POPUPS[row],
            file,
            file_name(file),
            180.0,
            FileKind::ALL
                .iter()
                .map(|f| (SharedString::from(file_name(*f)), *f))
                .collect(),
            move |ws, value| {
                ws.edit_smart_rules(|rules| {
                    if let Some(Rule::FileType { file }) = rule_at_mut(rules, &path) {
                        *file = value;
                    }
                })
            },
            cx,
        )
        .into_any_element(),
        Rule::HasLocation { yes } | Rule::Edited { yes } | Rule::HasVersions { yes } => choice(
            ws,
            VALUE_POPUPS[row],
            yes,
            yes_no(yes),
            90.0,
            vec![
                (SharedString::from(yes_no(true)), true),
                (SharedString::from(yes_no(false)), false),
            ],
            move |ws, value| {
                ws.edit_smart_rules(|rules| {
                    if let Some(
                        Rule::HasLocation { yes }
                        | Rule::Edited { yes }
                        | Rule::HasVersions { yes },
                    ) = rule_at_mut(rules, &path)
                    {
                        *yes = value;
                    }
                })
            },
            cx,
        )
        .into_any_element(),
        Rule::Person { name } => {
            // The people already named, when there are any; a name can
            // still be typed for someone not tagged yet.
            let known: Vec<String> = ws.library.people.iter().map(|p| p.name.clone()).collect();
            if known.is_empty() {
                text_field(
                    TEXT_FIELDS[row],
                    name,
                    t("common.name").to_string(),
                    220.0,
                    ws,
                    cx,
                )
                .into_any_element()
            } else {
                let label = if name.trim().is_empty() {
                    t("smart_album.choose_person").to_string()
                } else {
                    name.clone()
                };
                choice(
                    ws,
                    VALUE_POPUPS[row],
                    name,
                    label,
                    220.0,
                    known
                        .into_iter()
                        .map(|n| (SharedString::from(n.clone()), n))
                        .collect(),
                    move |ws, value| {
                        ws.edit_smart_rules(|rules| {
                            if let Some(Rule::Person { name }) = rule_at_mut(rules, &path) {
                                *name = value;
                            }
                        })
                    },
                    cx,
                )
                .into_any_element()
            }
        }
        Rule::Folder { path: folder } => {
            // Every folder the scan found, and the watched roots.
            let mut dirs: Vec<PathBuf> = ws.library.folders.clone();
            for section in &ws.library.sections {
                if !dirs.contains(&section.dir) {
                    dirs.push(section.dir.clone());
                }
            }
            dirs.sort();
            let label = if folder.as_os_str().is_empty() {
                t("smart_album.choose_folder").to_string()
            } else {
                crate::ui::shown_path(&folder)
            };
            choice(
                ws,
                VALUE_POPUPS[row],
                folder,
                label,
                260.0,
                dirs.into_iter()
                    .map(|d| (SharedString::from(crate::ui::shown_path(&d)), d))
                    .collect(),
                move |ws, value| {
                    ws.edit_smart_rules(|rules| {
                        if let Some(Rule::Folder { path }) = rule_at_mut(rules, &path) {
                            *path = value;
                        }
                    })
                },
                cx,
            )
            .into_any_element()
        }
        Rule::Group(group) => {
            let matching = group.matching;
            choice(
                ws,
                VALUE_POPUPS[row],
                matching,
                match_name(matching),
                200.0,
                Match::ALL
                    .iter()
                    .map(|m| (SharedString::from(match_name(*m)), *m))
                    .collect(),
                move |ws, value| {
                    ws.edit_smart_rules(|rules| {
                        if let Some(Rule::Group(group)) = rule_at_mut(rules, &path) {
                            group.matching = value;
                        }
                    })
                },
                cx,
            )
            .into_any_element()
        }
    }
}

/// The smart album editor: a name, how the rules combine, the rules —
/// one level of groups inside — and Create/Save.
pub(crate) fn smart_album_dialog(
    ws: &mut Workspace,
    editing: Option<usize>,
    name: String,
    rules: RuleGroup,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let fallback = match editing.and_then(|i| ws.library.buckets.get(i)) {
        Some(bucket) => bucket.name.clone(),
        None => tf!(
            "smart_album.default_name",
            n = ws.library.buckets.iter().filter(|b| b.is_album()).count() + 1
        ),
    };
    let name_field = super::library_view::bucket_field(NAME_FIELD, name, fallback, ws, cx);
    let top_matching = rules.matching;
    let total = rules.len();
    let full = total >= MAX_RULES;
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(crate::ui::field_row(t("common.name"), name_field))
        .child(crate::ui::field_row(
            t("smart_album.match_label"),
            choice(
                ws,
                TOP_MATCH_POPUP,
                top_matching,
                match_name(top_matching),
                220.0,
                Match::ALL
                    .iter()
                    .map(|m| (SharedString::from(match_name(*m)), *m))
                    .collect(),
                |ws, value| ws.edit_smart_rules(|rules| rules.matching = value),
                cx,
            ),
        ));
    let kinds: Vec<(SharedString, RuleKind)> = RuleKind::ROWS
        .iter()
        .map(|k| (SharedString::from(kind_name(*k)), *k))
        .collect();
    if rules.is_empty() {
        body = body.child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(crate::ui::palette().text_dim))
                .child(t("smart_album.empty")),
        );
    }
    for (row, path) in row_paths(&rules).into_iter().enumerate().take(MAX_RULES) {
        let Some(rule) = rule_at_mut(&mut rules.clone(), &path).map(|r| r.clone()) else {
            continue;
        };
        let depth = path.len() - 1;
        let kind = rule.kind();
        let kind_control = if kind == RuleKind::Group {
            div()
                .w(px(150.0))
                .text_size(px(12.0))
                .child(kind_name(RuleKind::Group))
                .into_any_element()
        } else {
            let p = path.clone();
            choice(
                ws,
                KIND_POPUPS[row],
                kind,
                kind_name(kind),
                150.0,
                kinds.clone(),
                move |ws, value| {
                    ws.edit_smart_rules(|rules| {
                        if let Some(rule) = rule_at_mut(rules, &p) {
                            if rule.kind() != value {
                                *rule = value.default_rule();
                            }
                        }
                    })
                },
                cx,
            )
            .into_any_element()
        };
        let remove_path = path.clone();
        let mut line = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .pl(px(16.0 * depth as f32))
            .child(kind_control)
            .child(rule_values(ws, row, path.clone(), &rule, cx))
            .child(div().flex_grow())
            .child(crate::ui::button(
                t("common.remove"),
                false,
                move |ws, _w, cx| {
                    let path = remove_path.clone();
                    ws.edit_smart_rules(|rules| {
                        let (last, parent) = path.split_last().expect("a row has a path");
                        let siblings = if parent.is_empty() {
                            Some(&mut rules.rules)
                        } else {
                            match rule_at_mut(rules, parent) {
                                Some(Rule::Group(g)) => Some(&mut g.rules),
                                _ => None,
                            }
                        };
                        if let Some(siblings) = siblings {
                            if *last < siblings.len() {
                                siblings.remove(*last);
                            }
                        }
                    });
                    cx.notify();
                },
                cx,
            ));
        if kind == RuleKind::Group && !full {
            let add_path = path.clone();
            line = line.child(crate::ui::button(
                t("smart_album.add_rule"),
                false,
                move |ws, _w, cx| {
                    let path = add_path.clone();
                    ws.edit_smart_rules(|rules| {
                        if let Some(Rule::Group(group)) = rule_at_mut(rules, &path) {
                            group.rules.push(RuleKind::Keyword.default_rule());
                        }
                    });
                    cx.notify();
                },
                cx,
            ));
        }
        body = body.child(line);
    }
    let mut adders = div().flex().flex_row().gap_2();
    if full {
        adders = adders.child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(crate::ui::palette().text_dim))
                .child(t("smart_album.limit")),
        );
    } else {
        adders = adders
            .child(crate::ui::button(
                t("smart_album.add_rule"),
                false,
                |ws, _w, cx| {
                    ws.edit_smart_rules(|rules| rules.rules.push(RuleKind::Rating.default_rule()));
                    cx.notify();
                },
                cx,
            ))
            .child(crate::ui::button(
                t("smart_album.add_group"),
                false,
                |ws, _w, cx| {
                    ws.edit_smart_rules(|rules| {
                        let mut group = RuleKind::Group.default_rule();
                        if let Rule::Group(g) = &mut group {
                            g.rules.push(RuleKind::Keyword.default_rule());
                        }
                        rules.rules.push(group);
                    });
                    cx.notify();
                },
                cx,
            ));
    }
    body = body.child(adders).child(
        div()
            .text_size(px(11.0))
            .text_color(gpui::rgb(pal().text_dim))
            .child(t("smart_album.help")),
    );
    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(crate::ui::button(
            t("common.cancel"),
            false,
            |ws, _w, cx| ws.close_modal(cx),
            cx,
        ))
        .child(crate::ui::button(
            if editing.is_some() {
                t("common.save")
            } else {
                t("smart_album.create")
            },
            true,
            |ws, _w, cx| ws.save_smart_album(cx),
            cx,
        ));
    let title = if editing.is_some() {
        t("smart_album.edit_title")
    } else {
        t("smart_album.new_title")
    };
    crate::ui::modal_frame(title, 680.0, body, actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_numbered_depth_first_and_fields_land_on_their_row() {
        let mut modal = Modal::SmartAlbum {
            editing: None,
            name: String::new(),
            rules: RuleGroup {
                matching: Match::All,
                rules: vec![
                    RuleKind::Keyword.default_rule(),
                    Rule::Group(RuleGroup {
                        matching: Match::Any,
                        rules: vec![
                            RuleKind::Camera.default_rule(),
                            RuleKind::Taken.default_rule(),
                        ],
                    }),
                    RuleKind::Person.default_rule(),
                ],
            },
        };
        let Modal::SmartAlbum { rules, .. } = &modal else {
            unreachable!()
        };
        assert_eq!(
            row_paths(rules),
            vec![vec![0], vec![1], vec![1, 0], vec![1, 1], vec![2]]
        );
        commit_field(&mut modal, NAME_FIELD, "Best of".into());
        commit_field(&mut modal, TEXT_FIELDS[0], "beach".into());
        commit_field(&mut modal, TEXT_FIELDS[2], "Canon".into());
        commit_field(&mut modal, TEXT_FIELDS[3], " 2024-01 ".into());
        commit_field(&mut modal, TO_FIELDS[3], "2024-06".into());
        commit_field(&mut modal, TEXT_FIELDS[4], "Ann".into());
        // A row that is not text takes nothing.
        commit_field(&mut modal, TEXT_FIELDS[1], "ignored".into());
        let Modal::SmartAlbum { name, rules, .. } = modal else {
            unreachable!()
        };
        assert_eq!(name, "Best of");
        assert_eq!(
            rules.rules[0],
            Rule::Keyword {
                text: "beach".into()
            }
        );
        let Rule::Group(inner) = &rules.rules[1] else {
            panic!("group kept")
        };
        assert_eq!(
            inner.rules[0],
            Rule::Camera {
                text: "Canon".into()
            }
        );
        assert_eq!(
            inner.rules[1],
            Rule::Taken {
                from: "2024-01".into(),
                to: "2024-06".into()
            }
        );
        assert_eq!(rules.rules[2], Rule::Person { name: "Ann".into() });
    }

    #[test]
    fn kept_versions_mark_only_their_own_photos() {
        let dir = std::env::temp_dir().join(format!("schist-smart-fill-{}", std::process::id()));
        let versions = dir.join(".schist").join("versions");
        std::fs::create_dir_all(&versions).unwrap();
        let kept = dir.join("kept.jpg");
        let sidecar = schist_gallery::backing_psd(&kept).unwrap();
        let sidecar_name = sidecar.file_name().unwrap().to_string_lossy().into_owned();
        std::fs::write(versions.join(format!("1700000000-{sidecar_name}")), b"").unwrap();
        let mut facts = vec![
            PhotoFacts {
                path: kept,
                ..Default::default()
            },
            PhotoFacts {
                path: dir.join("plain.jpg"),
                ..Default::default()
            },
        ];
        fill_versions(&mut facts);
        assert!(facts[0].has_versions);
        assert!(!facts[1].has_versions);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
