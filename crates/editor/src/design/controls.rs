//! Field targets are captured on focus. Typing edits a buffer; committing
//! applies one reversible operation to the original selection or style.
use super::DesignState;
use properties::ObjectProperty;
use schist_layout::{properties, LayoutObject, ObjectId};

#[derive(Default)]
pub struct Controls {
    pub swatch: Option<usize>,
    pub paragraph: Option<String>,
    pub character: Option<String>,
    pub object_style: Option<String>,
    pub field: Option<Target>,
    pub tab: usize,
    /// Disclosure state is chrome, not a document edit or an undo step.
    pub expanded: std::collections::HashSet<&'static str>,
    pub collapsed_layers: std::collections::HashSet<schist_layout::LayerId>,
}
#[derive(Clone)]
pub enum Target {
    TextPreferences,
    Swatch(schist_layout::Ink),
    Pages(Vec<usize>),
    Section(usize),
    Objects(Vec<ObjectId>),
    Paragraph(String),
    /// A tab edit captures both its style and record. If another operation
    /// replaces that record before commit, it must not edit its new occupant.
    Tab {
        paragraph: String,
        index: usize,
        original: schist_layout::lists::ListTab,
    },
    Character(String),
    ObjectStyle(String),
}

pub fn page_property(id: &str) -> Option<properties::PageProperty> {
    use properties::PageProperty::*;
    Some(match id {
        "design-prop-page-width" | "design-prop-bar-page-width" => Width,
        "design-prop-page-height" | "design-prop-bar-page-height" => Height,
        "design-prop-bleed" => Bleed,
        "design-prop-slug" => Slug,
        "design-prop-bleed-top" => BleedTop,
        "design-prop-bleed-bottom" => BleedBottom,
        "design-prop-bleed-inside" => BleedInside,
        "design-prop-bleed-outside" => BleedOutside,
        "design-prop-slug-top" => SlugTop,
        "design-prop-slug-bottom" => SlugBottom,
        "design-prop-slug-inside" => SlugInside,
        "design-prop-slug-outside" => SlugOutside,
        "design-prop-margin-top" => MarginTop,
        "design-prop-margin-right" => MarginRight,
        "design-prop-margin-bottom" => MarginBottom,
        "design-prop-margin-left" => MarginLeft,
        _ => return None,
    })
}

pub fn object_property(id: &str) -> Option<ObjectProperty> {
    Some(match id {
        "design-prop-x" => ObjectProperty::X,
        "design-prop-y" => ObjectProperty::Y,
        "design-prop-width" => ObjectProperty::Width,
        "design-prop-height" => ObjectProperty::Height,
        "design-prop-columns" => ObjectProperty::Columns,
        "design-prop-gutter" => ObjectProperty::Gutter,
        "design-prop-inset" => ObjectProperty::Inset,
        "design-prop-fill-tint" => ObjectProperty::FillTint,
        "design-prop-stroke-tint" => ObjectProperty::StrokeTint,
        "design-prop-stroke-width" => ObjectProperty::StrokeWidth,
        _ => return None,
    })
}

/// Paint selection is one style edit. None inherits; Paint::None explicitly
/// suppresses inherited ink, and choosing a color re-enables that paint.
pub fn set_text_paint(
    state: &mut DesignState,
    target: &Target,
    fill: bool,
    paint: Option<schist_layout::Paint>,
) -> bool {
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        let fields =
            match target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| {
                        if fill {
                            (&mut s.fill, &mut s.fill_disabled)
                        } else {
                            (&mut s.stroke, &mut s.stroke_disabled)
                        }
                    }),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| {
                        if fill {
                            (&mut s.fill, &mut s.fill_disabled)
                        } else {
                            (&mut s.stroke, &mut s.stroke_disabled)
                        }
                    }),
                _ => None,
            };
        if let Some((ink, disabled)) = fields {
            *disabled = paint == Some(schist_layout::Paint::None);
            *ink = paint.as_ref().and_then(schist_layout::Paint::ink).cloned();
        }
    })
}

pub fn set_text_paint_flag(
    state: &mut DesignState,
    target: &Target,
    flag: &str,
    value: Option<bool>,
) -> bool {
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        let fields =
            match target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| {
                        (
                            &mut s.overprint_fill,
                            &mut s.overprint_stroke,
                            &mut s.stroke_outside,
                        )
                    }),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| {
                        (
                            &mut s.overprint_fill,
                            &mut s.overprint_stroke,
                            &mut s.stroke_outside,
                        )
                    }),
                _ => None,
            };
        if let Some((fill, stroke, outside)) = fields {
            match flag {
                "fill" => *fill = value,
                "stroke" => *stroke = value,
                "outside" => *outside = value,
                _ => {}
            }
        }
    })
}

pub fn set_capitalization(
    state: &mut DesignState,
    target: &Target,
    value: Option<schist_text_engine::Capitalization>,
) -> bool {
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        let fields = match target {
            Target::Paragraph(name) => styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == *name)
                .map(|s| (&mut s.all_caps, &mut s.small_caps)),
            Target::Character(name) => styles
                .characters
                .iter_mut()
                .find(|s| s.name == *name)
                .map(|s| (&mut s.all_caps, &mut s.small_caps)),
            _ => None,
        };
        if let Some((all, small)) = fields {
            *all = value.map(|v| v.flags().0);
            *small = value.map(|v| v.flags().1);
        }
    })
}

pub fn set_directional_feature(
    state: &mut DesignState,
    target: &Target,
    proportional: bool,
    value: Option<bool>,
) -> bool {
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        let fields = match target {
            Target::Paragraph(name) => styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == *name)
                .map(|s| (&mut s.directional_features, &mut s.features)),
            Target::Character(name) => styles
                .characters
                .iter_mut()
                .find(|s| s.name == *name)
                .map(|s| (&mut s.directional_features, &mut s.features)),
            _ => None,
        };
        if let Some((mode, features)) = fields {
            let (field, tags) = if proportional {
                (&mut mode.proportional_metrics, ["palt", "vpal"])
            } else {
                (&mut mode.kana, ["hkna", "vkna"])
            };
            *field = value;
            // Choosing a mode default also clears same-level tag exceptions;
            // otherwise an Enabled/Disabled choice could appear to do nothing.
            features.retain(|(tag, _)| !tags.contains(&tag.as_str()));
        }
    })
}

pub fn set_text_join(
    state: &mut DesignState,
    target: &Target,
    value: Option<schist_text_engine::TextStrokeJoin>,
) -> bool {
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        let field = match target {
            Target::Paragraph(name) => styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == *name)
                .map(|s| &mut s.stroke_join),
            Target::Character(name) => styles
                .characters
                .iter_mut()
                .find(|s| s.name == *name)
                .map(|s| &mut s.stroke_join),
            _ => None,
        };
        if let Some(field) = field {
            *field = value;
        }
    })
}

pub fn edit_decoration(
    state: &mut DesignState,
    target: &Target,
    strike: bool,
    edit: impl FnOnce(&mut schist_layout::decorations::DecorationStyle),
) -> bool {
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        let field =
            match target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| {
                        if strike {
                            &mut s.strike_style
                        } else {
                            &mut s.underline_style
                        }
                    }),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| {
                        if strike {
                            &mut s.strike_style
                        } else {
                            &mut s.underline_style
                        }
                    }),
                _ => None,
            };
        if let Some(field) = field {
            edit(field);
        }
    })
}

pub fn set_decoration_enabled(
    state: &mut DesignState,
    target: &Target,
    strike: bool,
    value: Option<bool>,
) -> bool {
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        let field =
            match target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| {
                        if strike {
                            &mut s.strikethrough
                        } else {
                            &mut s.underline
                        }
                    }),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| {
                        if strike {
                            &mut s.strikethrough
                        } else {
                            &mut s.underline
                        }
                    }),
                _ => None,
            };
        if let Some(field) = field {
            *field = value;
        }
    })
}

/// Caps belong to the selected native dash definition; one resource edit undoes once.
pub fn set_decoration_cap(
    state: &mut DesignState,
    target: &Target,
    strike: bool,
    cap: schist_text_engine::DecorationCap,
) -> bool {
    edit_decoration(state, target, strike, |d| {
        if let Some(schist_layout::decorations::DecorationStroke {
            pattern: schist_text_engine::TextDecorationPattern::Dashes(dashes),
            ..
        }) = &mut d.stroke
        {
            dashes.cap = cap;
        }
    })
}

/// Fitting is part of the named resource, captured and changed in one edit.
pub fn set_decoration_fitting(
    state: &mut DesignState,
    target: &Target,
    strike: bool,
    fitting: schist_text_engine::DecorationFit,
) -> bool {
    edit_decoration(state, target, strike, |d| {
        if let Some(stroke) = &mut d.stroke {
            let mut changed = stroke.clone();
            changed.fitting = fitting;
            if changed.valid() {
                *stroke = changed;
            }
        }
    })
}

fn decoration_field(
    state: &mut DesignState,
    target: &Target,
    strike: bool,
    field: &str,
    text: &str,
) -> bool {
    use schist_layout::decorations::{DecorationMeasure, DecorationPaint};
    if matches!(field, "stripes" | "dashes" | "dots") {
        use schist_text_engine::TextDecorationPattern;
        let same_kind = |p: &TextDecorationPattern| {
            matches!(
                (p, field),
                (TextDecorationPattern::Dashes(_), "dashes")
                    | (TextDecorationPattern::Stripes(_), "stripes")
                    | (TextDecorationPattern::Dots(_), "dots")
            )
        };
        let pattern = if text.is_empty() {
            None
        } else {
            let Ok(values) = text
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<Vec<f32>, _>>()
            else {
                return false;
            };
            let pattern = match field {
                "dashes" => TextDecorationPattern::Dashes(values.into()),
                "dots" => TextDecorationPattern::Dots(values),
                _ => TextDecorationPattern::Stripes(values),
            };
            if !pattern.valid() {
                return false;
            }
            Some(pattern)
        };
        return edit_decoration(state, target, strike, |d| {
            if let Some(mut pattern) = pattern {
                if let (
                    TextDecorationPattern::Dashes(new),
                    Some(schist_layout::decorations::DecorationStroke {
                        pattern: TextDecorationPattern::Dashes(old),
                        ..
                    }),
                ) = (&mut pattern, &d.stroke)
                {
                    new.cap = old.cap;
                }
                if d.stroke.as_ref().is_some_and(|s| s.pattern == pattern) {
                    return;
                }
                d.stroke = Some(schist_layout::decorations::DecorationStroke {
                    fitting: d
                        .stroke
                        .as_ref()
                        .filter(|s| same_kind(&s.pattern))
                        .map(|s| s.fitting)
                        .unwrap_or_default(),
                    name: d
                        .stroke
                        .as_ref()
                        .filter(|s| same_kind(&s.pattern))
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| {
                            schist_i18n::t(match field {
                                "dashes" => "design.decoration_dashed",
                                "dots" => "design.decoration_dotted",
                                _ => "design.decoration_striped",
                            })
                            .to_string()
                        }),
                    pattern,
                });
            } else if d.stroke.as_ref().is_some_and(|s| same_kind(&s.pattern)) {
                // Other patterns have no value in this field. Focusing a blank
                // must not restore inheritance; the pattern picker does that.
                d.stroke = None;
            }
        });
    }
    if field == "weight" || field == "offset" {
        let value = if text.is_empty() {
            None
        } else if text.eq_ignore_ascii_case("auto")
            || text.eq_ignore_ascii_case(schist_i18n::t("design.leading_auto"))
        {
            Some(DecorationMeasure::Auto)
        } else {
            match text.parse::<f32>() {
                Ok(value)
                    if value.is_finite()
                        && value != -9999.0
                        && (field != "weight" || value >= 0.0) =>
                {
                    Some(DecorationMeasure::Points(value))
                }
                _ => return false,
            }
        };
        return edit_decoration(state, target, strike, |d| {
            if field == "weight" {
                d.weight = value;
            } else {
                d.offset = value;
            }
        });
    }
    if field == "tint" || field == "gap-tint" {
        let value = if text.is_empty() {
            None
        } else {
            match text.parse::<f32>() {
                Ok(v) if v.is_finite() && (0.0..=100.0).contains(&v) => Some(v / 100.0),
                _ => return false,
            }
        };
        let resolved = match target {
            Target::Paragraph(name) => {
                let s = state.document.styles.resolve_paragraph(name);
                if strike {
                    s.strike_style
                } else {
                    s.underline_style
                }
            }
            Target::Character(name) => {
                let s = state.document.styles.resolve_character(name);
                if strike {
                    s.strike_style
                } else {
                    s.underline_style
                }
            }
            _ => return false,
        };
        let gap = field == "gap-tint";
        let detached = (if gap {
            resolved.gap_paint.as_ref()
        } else {
            resolved.paint.as_ref()
        })
        .and_then(DecorationPaint::ink)
        .filter(|i| i.tint.is_some())
        .map(|ink| DecorationPaint::Ink(ink.base_color().into_owned()));
        return edit_decoration(state, target, strike, |d| {
            if gap {
                d.gap_tint = value;
            } else {
                d.tint = value;
            }
            if value.is_some() && detached.is_some() {
                if gap {
                    d.gap_paint = detached;
                } else {
                    d.paint = detached;
                }
            }
        });
    }
    false
}

/// Position and explicit baseline offset are independent. Choosing Normal
/// resets inherited positioning, while None restores it. One style edit undoes it.
pub fn set_position(
    state: &mut DesignState,
    target: &Target,
    position: Option<schist_layout::styles::TextPosition>,
) -> bool {
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        let fields = match target {
            Target::Paragraph(name) => styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == *name)
                .map(|s| (&mut s.position, &mut s.baseline_shift)),
            Target::Character(name) => styles
                .characters
                .iter_mut()
                .find(|s| s.name == *name)
                .map(|s| (&mut s.position, &mut s.baseline_shift)),
            _ => None,
        };
        if let Some((field, shift)) = fields {
            *field = position;
            // Remove only legacy conflated script state; keep numeric offsets.
            if matches!(
                shift,
                Some(
                    schist_layout::styles::BaselineShift::Superscript
                        | schist_layout::styles::BaselineShift::Subscript
                )
            ) {
                *shift = None;
            }
        }
    })
}

/// Show explicit legacy face choices too; blank must mean inheritance.
pub fn font_style_value(styles: &schist_layout::StyleSet, target: &Target) -> String {
    let (named, bold, italic, resolved) = match target {
        Target::Paragraph(name) => {
            let Some(s) = styles.paragraph(name) else {
                return String::new();
            };
            let r = styles.resolve_paragraph(name);
            (&s.font_style, s.bold, s.italic, (r.bold, r.italic))
        }
        Target::Character(name) => {
            let Some(s) = styles.character(name) else {
                return String::new();
            };
            let r = styles.resolve_character(name);
            (&s.font_style, s.bold, s.italic, (r.bold, r.italic))
        }
        _ => return String::new(),
    };
    if let Some(name) = named {
        return name.clone();
    }
    if bold.is_none() && italic.is_none() {
        return String::new();
    }
    match (resolved.0.unwrap_or(false), resolved.1.unwrap_or(false)) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        _ => "Regular",
    }
    .into()
}

/// Display a tag while retaining the identity/dictionary of an unchanged
/// imported resource. An explicit empty language is the default, not inheritance.
pub fn language_value(styles: &schist_layout::StyleSet, target: &Target) -> String {
    let value = match target {
        Target::Paragraph(name) => styles.paragraph(name).and_then(|s| s.language.as_ref()),
        Target::Character(name) => styles.character(name).and_then(|s| s.language.as_ref()),
        _ => None,
    };
    match value {
        None => String::new(),
        Some(value) if value.as_str().is_empty() => "und".into(),
        Some(value) => styles
            .resolve_language(value)
            .unwrap_or_else(|| value.as_str().trim_start_matches("$ID/").to_owned()),
    }
}

/// List fields edit the style captured at focus, in one history operation.
pub fn edit_list(
    state: &mut DesignState,
    target: &Target,
    edit: impl FnOnce(&mut schist_layout::lists::ListStyle),
) -> bool {
    let Target::Paragraph(name) = target else {
        return false;
    };
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        if let Some(style) = styles.paragraphs.iter_mut().find(|s| s.name == *name) {
            edit(&mut style.list);
        }
    })
}

pub fn list_bullet_value(list: &schist_layout::lists::ListStyle) -> String {
    list.bullet
        .as_ref()
        .map(|b| {
            b.character()
                .map(|v| v.to_string())
                .unwrap_or_else(|| format!("{}:{}", b.kind, b.value))
        })
        .unwrap_or_default()
}

pub fn list_tab_value(list: &schist_layout::lists::ListStyle) -> String {
    list.tabs
        .as_ref()
        .map(|tabs| {
            tabs.iter()
                .map(|t| t.position.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

pub fn edit_tab(
    state: &mut DesignState,
    target: &Target,
    edit: impl FnOnce(&mut Vec<schist_layout::lists::ListTab>, usize),
) -> bool {
    let Target::Tab {
        paragraph,
        index,
        original,
    } = target
    else {
        return false;
    };
    let mut tabs = state
        .document
        .styles
        .resolve_paragraph(paragraph)
        .list
        .tabs
        .unwrap_or_default();
    if tabs.get(*index) != Some(original) {
        return false;
    }
    let before = tabs.clone();
    edit(&mut tabs, *index);
    if tabs == before {
        return false;
    }
    edit_list(state, &Target::Paragraph(paragraph.clone()), |list| {
        list.tabs = Some(tabs)
    })
}

pub fn tab_target(state: &DesignState, paragraph: &str, index: usize) -> Option<Target> {
    let original = state
        .document
        .styles
        .resolve_paragraph(paragraph)
        .list
        .tabs?
        .get(index)?
        .clone();
    Some(Target::Tab {
        paragraph: paragraph.into(),
        index,
        original,
    })
}

pub fn add_tab(state: &mut DesignState, target: &Target) -> bool {
    let Target::Paragraph(name) = target else {
        return false;
    };
    let mut tabs = state
        .document
        .styles
        .resolve_paragraph(name)
        .list
        .tabs
        .unwrap_or_default();
    if tabs.iter().any(|tab| !tab.position.is_finite()) {
        return false;
    }
    let last = tabs.iter().fold(0.0_f32, |p, tab| p.max(tab.position));
    let position = last + 36.0;
    if !position.is_finite() || position <= last {
        return false;
    }
    tabs.push(schist_layout::lists::ListTab {
        position,
        alignment: "LeftAlign".into(),
        alignment_character: ".".into(),
        leader: String::new(),
    });
    edit_list(state, target, |list| list.tabs = Some(tabs))
}

pub fn set_tab_alignment(
    state: &mut DesignState,
    target: &Target,
    alignment: &'static str,
) -> bool {
    if !matches!(
        alignment,
        "LeftAlign" | "CenterAlign" | "RightAlign" | "CharacterAlign"
    ) {
        return false;
    }
    edit_tab(state, target, |tabs, index| {
        let tab = &mut tabs[index];
        tab.alignment = alignment.into();
        if alignment == "CharacterAlign" && tab.text_alignment().is_none() {
            tab.alignment_character = ".".into();
        }
    })
}

fn commit_tab(state: &mut DesignState, target: &Target, id: &str, text: &str) -> bool {
    match id {
        "design-prop-tab-position" => {
            let Some(position) = text
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite() && *v >= 0.0)
            else {
                return false;
            };
            edit_tab(state, target, |tabs, index| tabs[index].position = position)
        }
        "design-prop-tab-character" => {
            let mut chars = text.chars();
            let Some(character) = chars
                .next()
                .filter(|c| !c.is_control() && !matches!(c, '\u{2028}' | '\u{2029}'))
            else {
                return false;
            };
            if chars.next().is_some() {
                return false;
            }
            edit_tab(state, target, |tabs, index| {
                tabs[index].alignment_character = character.into()
            })
        }
        _ => false,
    }
}

pub fn set_list_kind(
    state: &mut DesignState,
    target: &Target,
    kind: Option<schist_layout::lists::ListKind>,
) -> bool {
    let Target::Paragraph(name) = target else {
        return false;
    };
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        if let Some(style) = styles.paragraphs.iter_mut().find(|s| s.name == *name) {
            style.bullet = None;
            style.list.kind = kind;
        }
    })
}

pub fn set_list_format(
    state: &mut DesignState,
    target: &Target,
    format: Option<schist_layout::list_numbering::CounterFormat>,
) -> bool {
    edit_list(state, target, |list| {
        // Picking the displayed format must not rewrite an imported named
        // definition to an enumeration or erase its original spelling.
        if list.format.is_none() && format.is_none()
            || format.is_some() && list.format.as_ref().and_then(|f| f.counter_format()) == format
        {
            return;
        }
        list.format = format.map(|f| f.native());
    })
}

/// Change the captured paragraph's restart behavior in one undoable edit.
/// Disabling retains an imported policy; choosing inheritance clears both fields.
pub fn set_list_restart_policy(
    state: &mut DesignState,
    target: &Target,
    enabled: Option<bool>,
) -> bool {
    edit_list(state, target, |list| {
        list.apply_restart_policy = enabled;
        match enabled {
            None => list.restart_policy = None,
            Some(true) => {
                list.restart_policy = Some(schist_layout::lists::RestartPolicy {
                    policy: "AnyPreviousLevel".into(),
                    lower: 0,
                    upper: 0,
                })
            }
            Some(false) => {}
        }
    })
}

fn commit_list(state: &mut DesignState, target: &Target, id: &str, text: &str) -> bool {
    use schist_layout::lists::{BulletSymbol, ListTab};
    let raw = text.trim();
    match id {
        "design-prop-list-level" => {
            let value = if raw.is_empty() {
                None
            } else {
                match raw.parse::<u32>() {
                    Ok(v) if (1..=9).contains(&v) => Some(v),
                    _ => return false,
                }
            };
            edit_list(state, target, |list| list.level = value)
        }
        "design-prop-list-start" => {
            let value = if raw.is_empty() {
                None
            } else {
                match raw.parse::<u32>() {
                    Ok(v) if v > 0 && v <= i32::MAX as u32 => Some(v),
                    _ => return false,
                }
            };
            edit_list(state, target, |list| list.start = value)
        }
        "design-prop-list-bullet" => {
            let Target::Paragraph(name) = target else {
                return false;
            };
            if state
                .document
                .styles
                .paragraph(name)
                .is_some_and(|s| raw == list_bullet_value(&s.list))
            {
                return false;
            }
            let value = if raw.is_empty() {
                None
            } else {
                let mut chars = raw.chars();
                let Some(c) = chars.next().filter(|c| !c.is_control()) else {
                    return false;
                };
                if chars.next().is_some() {
                    return false;
                }
                Some(BulletSymbol::unicode(c))
            };
            edit_list(state, target, |list| list.bullet = value)
        }
        "design-prop-list-tab" => {
            let Target::Paragraph(name) = target else {
                return false;
            };
            if state
                .document
                .styles
                .paragraph(name)
                .is_some_and(|s| raw == list_tab_value(&s.list))
            {
                return false;
            }
            let value = if raw.is_empty() {
                None
            } else {
                let Some(values) = raw
                    .split(',')
                    .map(|v| {
                        v.trim()
                            .parse::<f32>()
                            .ok()
                            .filter(|v| v.is_finite() && *v >= 0.0)
                    })
                    .collect::<Option<Vec<_>>>()
                else {
                    return false;
                };
                let existing = state
                    .document
                    .styles
                    .resolve_paragraph(name)
                    .list
                    .tabs
                    .unwrap_or_default();
                Some(
                    values
                        .into_iter()
                        .enumerate()
                        .map(|(index, position)| ListTab {
                            position,
                            ..existing.get(index).cloned().unwrap_or_else(|| ListTab {
                                position: 0.0,
                                alignment: "LeftAlign".into(),
                                alignment_character: ".".into(),
                                leader: String::new(),
                            })
                        })
                        .collect(),
                )
            };
            edit_list(state, target, |list| list.tabs = value)
        }
        "design-prop-list-expression" => {
            if text.chars().any(char::is_control) {
                return false;
            }
            let value = (!text.is_empty()).then(|| text.to_owned());
            let Target::Paragraph(name) = target else {
                return false;
            };
            let level = state.document.styles.resolve_paragraph(name).list.level;
            let probe = schist_layout::lists::ListStyle {
                level,
                kind: Some(schist_layout::lists::ListKind::Numbered),
                expression: value.clone(),
                ..Default::default()
            };
            if !schist_layout::list_composition::unsupported(&probe).is_empty() {
                return false;
            }
            edit_list(state, target, |list| list.expression = value)
        }
        _ => false,
    }
}

pub fn commit(state: &mut DesignState, id: &str, text: &str) -> bool {
    let Some(target) = state.controls.field.take() else {
        return false;
    };
    if matches!(target, Target::Tab { .. }) {
        return commit_tab(state, &target, id, text);
    }
    if id.starts_with("design-prop-list-") {
        return commit_list(state, &target, id, text);
    }
    if matches!(
        id,
        "design-prop-paragraph-language" | "design-prop-language"
    ) {
        let text = text.trim();
        if text == language_value(&state.document.styles, &target) {
            return false;
        }
        let value = if text.is_empty() {
            None
        } else {
            let Some(tag) = schist_text_engine::normalize_language(text) else {
                return false;
            };
            Some(schist_layout::language::TextLanguage::Tag { tag })
        };
        return properties::edit_styles(&mut state.document, &mut state.history, |styles| {
            let field = match &target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.language),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.language),
                _ => None,
            };
            if let Some(field) = field {
                *field = value;
            }
        });
    }
    for (prefix, strike) in [
        ("design-prop-para-underline-", false),
        ("design-prop-para-strike-", true),
        ("design-prop-char-underline-", false),
        ("design-prop-char-strike-", true),
    ] {
        if let Some(field) = id.strip_prefix(prefix) {
            return decoration_field(state, &target, strike, field, text.trim());
        }
    }
    if let Target::ObjectStyle(name) = &target {
        let text = text.trim();
        if id == "design-prop-object-name" {
            let changed = schist_layout::object_styles::rename_style(
                &mut state.document,
                &mut state.history,
                name,
                text,
            );
            if changed {
                state.controls.object_style = Some(text.into());
            }
            return changed;
        }
        if id == "design-prop-object-base" {
            if !text.is_empty() {
                let mut next = Some(text);
                let mut seen = std::collections::HashSet::new();
                while let Some(base) = next {
                    if base == name || !seen.insert(base) {
                        return false;
                    }
                    let Some(style) = state.document.styles.object_style(base) else {
                        return false;
                    };
                    next = style.based_on.as_deref();
                }
            }
            return properties::edit_styles(&mut state.document, &mut state.history, |styles| {
                if let Some(style) = styles.objects.iter_mut().find(|s| s.name == *name) {
                    style.based_on = (!text.is_empty()).then(|| text.into());
                }
            });
        }
        let value = if text.is_empty() {
            None
        } else {
            match text.parse::<f32>() {
                Ok(v) if v.is_finite() && v >= 0.0 => Some(v),
                _ => return false,
            }
        };
        if id.ends_with("tint") && value.is_some_and(|v| v > 100.0) {
            return false;
        }
        let resolved = state.document.styles.resolve_object(name).paint;
        return properties::edit_styles(&mut state.document, &mut state.history, |styles| {
            let Some(style) = styles.objects.iter_mut().find(|s| s.name == *name) else {
                return;
            };
            match id {
                "design-prop-object-fill-tint" => {
                    style.paint.fill_tint = value.map(|v| v / 100.0);
                    if value.is_some() {
                        if let Some(ink) = resolved.fill_ink().filter(|i| i.tint.is_some()) {
                            style.paint.fill =
                                Some(schist_layout::Paint::Ink(ink.base_color().into_owned()));
                        }
                    }
                }
                "design-prop-object-stroke-tint" => {
                    style.paint.stroke_tint = value.map(|v| v / 100.0);
                    if value.is_some() {
                        if let Some(ink) = resolved.stroke_ink().filter(|i| i.tint.is_some()) {
                            style.paint.stroke =
                                Some(schist_layout::Paint::Ink(ink.base_color().into_owned()));
                        }
                    }
                }
                "design-prop-object-stroke-width" => style.paint.stroke_width = value,
                _ => {}
            }
        });
    }
    if matches!(
        id,
        "design-prop-font-style" | "design-prop-paragraph-font-style"
    ) {
        let name = text.trim();
        // Focusing and leaving a legacy face must not turn its independent
        // inheritance into a new atomic named override.
        if name == font_style_value(&state.document.styles, &target) {
            return false;
        }
        if name.chars().any(char::is_control) {
            return false;
        }
        return properties::edit_styles(&mut state.document, &mut state.history, |styles| {
            let fields = match &target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| (&mut s.font_style, &mut s.bold, &mut s.italic)),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| (&mut s.font_style, &mut s.bold, &mut s.italic)),
                _ => None,
            };
            if let Some((style, bold, italic)) = fields {
                *style = (!name.is_empty()).then(|| name.to_owned());
                // Blank restores face inheritance, including old imported flags.
                *bold = None;
                *italic = None;
            }
        });
    }
    if matches!(id, "design-prop-leading" | "design-prop-char-leading") {
        use schist_layout::styles::Leading;
        let text = text.trim();
        let leading = if text.is_empty() {
            None
        } else if text.eq_ignore_ascii_case("Auto")
            || text.eq_ignore_ascii_case(schist_i18n::t("design.leading_auto"))
        {
            Some(Leading::Auto)
        } else {
            match text.parse::<f32>() {
                Ok(v) if v.is_finite() && v >= 0.0 => Some(Leading::Points(v)),
                _ => return false,
            }
        };
        return properties::edit_styles(&mut state.document, &mut state.history, |styles| {
            let field = match &target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.leading),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.leading),
                _ => None,
            };
            if let Some(field) = field {
                *field = leading;
            }
        });
    }
    if matches!(
        id,
        "design-prop-paragraph-features" | "design-prop-char-features"
    ) {
        let Some(features) = parse_features(text) else {
            return false;
        };
        return properties::edit_styles(&mut state.document, &mut state.history, |styles| {
            let field = match &target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.features),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.features),
                _ => None,
            };
            if let Some(field) = field {
                *field = features;
            }
        });
    }
    if let Target::Section(page) = target {
        if matches!(
            id,
            "design-prop-number-prefix" | "design-prop-section-name" | "design-prop-section-marker"
        ) {
            return schist_layout::numbering::edit_section(
                &mut state.document,
                &mut state.history,
                page,
                |section| match id {
                    "design-prop-number-prefix" => section.prefix = text.into(),
                    "design-prop-section-name" => section.name = text.into(),
                    "design-prop-section-marker" => section.marker = text.into(),
                    _ => unreachable!(),
                },
            );
        }
    }
    if id == "design-prop-family" || id == "design-prop-paragraph-family" {
        let family = text.trim().to_owned();
        return properties::edit_styles(&mut state.document, &mut state.history, |styles| {
            let field = match &target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.family),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.family),
                _ => None,
            };
            if let Some(field) = field {
                *field = (!family.is_empty()).then_some(family);
            }
        });
    }
    if matches!(id, "design-prop-style-name" | "design-prop-char-name") {
        let (paragraph, name) = match &target {
            Target::Paragraph(name) => (true, name),
            Target::Character(name) => (false, name),
            _ => return false,
        };
        let changed = properties::rename_style(
            &mut state.document,
            &mut state.history,
            paragraph,
            name,
            text,
        );
        if changed {
            if paragraph {
                state.controls.paragraph = Some(text.trim().into());
            } else {
                state.controls.character = Some(text.trim().into());
            }
        }
        return changed;
    }
    let value = if text.trim().is_empty() {
        None
    } else {
        match text.trim().parse::<f32>() {
            Ok(value) if value.is_finite() => Some(value),
            _ => return false,
        }
    };
    if (id.ends_with("stroke-width") || id.ends_with("stroke-miter"))
        && value.is_some_and(|v| v < 0.0)
    {
        return false;
    }
    if id.ends_with("-tint") && value.is_some_and(|v| !(0.0..=100.0).contains(&v)) {
        return false;
    }
    match target {
        Target::ObjectStyle(_) => false,
        Target::TextPreferences => {
            let Some(value) = value else {
                return false;
            };
            let size = matches!(
                id,
                "design-prop-superscript-size"
                    | "design-prop-subscript-size"
                    | "design-prop-small-cap-size"
            );
            let range = if size { 1.0..=200.0 } else { -500.0..=500.0 };
            if !range.contains(&value) {
                return false;
            }
            properties::edit_styles(&mut state.document, &mut state.history, |styles| {
                let prefs = &mut styles.text_preferences;
                match id {
                    "design-prop-small-cap-size" => prefs.small_cap_size = value,
                    "design-prop-superscript-size" => prefs.superscript_size = value,
                    "design-prop-superscript-position" => prefs.superscript_position = value,
                    "design-prop-subscript-size" => prefs.subscript_size = value,
                    "design-prop-subscript-position" => prefs.subscript_position = value,
                    _ => {}
                }
            })
        }
        Target::Swatch(ink) => {
            let Some(value) = value else {
                return false;
            };
            let mut after;
            if id == "design-prop-swatch-tint" {
                return schist_layout::swatches::set_tint(
                    &mut state.document,
                    &mut state.history,
                    &ink,
                    value / 100.0,
                );
            } else if id.starts_with("design-prop-swatch-cmyk-") {
                let channel = match id {
                    "design-prop-swatch-cmyk-c" => 0,
                    "design-prop-swatch-cmyk-m" => 1,
                    "design-prop-swatch-cmyk-y" => 2,
                    "design-prop-swatch-cmyk-k" => 3,
                    _ => return false,
                };
                let Some(mut cmyk) = ink.source_cmyk else {
                    return false;
                };
                if ink.tint.is_some() || !(0.0..=100.0).contains(&value) {
                    return false;
                }
                cmyk[channel] = value / 100.0;
                after = schist_layout::Ink::cmyk(ink.name.clone(), cmyk);
                after.spot = ink.spot;
            } else {
                let channel = match id {
                    "design-prop-swatch-red" => 0,
                    "design-prop-swatch-green" => 1,
                    "design-prop-swatch-blue" => 2,
                    _ => return false,
                };
                if ink.tint.is_some() || !(0.0..=255.0).contains(&value) {
                    return false;
                }
                let mut rgb = ink.preview_rgb;
                rgb[channel] = value / 255.0;
                after = schist_layout::Ink::process(ink.name.clone(), rgb);
                after.spot = ink.spot;
            }
            schist_layout::swatches::replace(&mut state.document, &mut state.history, &ink, after)
        }
        Target::Pages(pages) => value.is_some_and(|value| {
            page_property(id).is_some_and(|property| {
                properties::set_page_property(
                    &mut state.document,
                    &mut state.history,
                    &pages,
                    property,
                    value,
                )
            })
        }),
        Target::Section(page) => {
            if id != "design-prop-number-start" {
                return false;
            }
            let Some(value) = value.filter(|v| (1.0..=999999.0).contains(v) && v.fract() == 0.0)
            else {
                return false;
            };
            schist_layout::numbering::edit_section(
                &mut state.document,
                &mut state.history,
                page,
                |section| {
                    section.start = value as u32;
                    section.continue_numbering = false;
                },
            )
        }
        Target::Objects(ids) if matches!(id, "design-prop-path-start" | "design-prop-path-end") => {
            use schist_layout::text_path::{set_bracket, Bracket};
            let values: Option<Vec<_>> = ids
                .iter()
                .map(|object| match &state.document.object(*object)?.object {
                    LayoutObject::TextFrame {
                        text_path: Some(path),
                        ..
                    } => Some(if id == "design-prop-path-start" {
                        Some(path.start)
                    } else {
                        path.end
                    }),
                    _ => None,
                })
                .collect();
            if values.as_ref().is_some_and(|values| {
                values.first().is_some_and(|first| {
                    values.iter().all(|value| value == first)
                        && first.map(|v| format!("{v:.2}")).unwrap_or_default() == text.trim()
                })
            }) {
                return false;
            }
            let bracket = if id == "design-prop-path-start" {
                let Some(value) = value else {
                    return false;
                };
                Bracket::Start(value)
            } else {
                if !text.trim().is_empty() && value.is_none() {
                    return false;
                }
                Bracket::End(value)
            };
            set_bracket(&mut state.document, &mut state.history, &ids, bracket)
        }
        Target::Objects(ids) => value.is_some_and(|value| {
            object_property(id).is_some_and(|property| {
                properties::set_object_property(
                    &mut state.document,
                    &mut state.history,
                    &ids,
                    property,
                    value,
                )
            })
        }),
        Target::Paragraph(name) => {
            let tint_base = state
                .document
                .styles
                .resolve_paragraph(&name)
                .character(
                    state
                        .document
                        .styles
                        .resolve_character(&state.document.default_character_style),
                )
                .fill
                .filter(|ink| ink.tint.is_some())
                .map(|ink| ink.base_color().into_owned());
            let stroke_tint_base = state
                .document
                .styles
                .resolve_paragraph(&name)
                .character(
                    state
                        .document
                        .styles
                        .resolve_character(&state.document.default_character_style),
                )
                .stroke
                .filter(|ink| ink.tint.is_some())
                .map(|ink| ink.base_color().into_owned());
            if id == "design-prop-size" && value.is_some_and(|value| value <= 0.0) {
                return false;
            }
            if id == "design-prop-auto-leading"
                && value.is_some_and(|v| !(0.0..=500.0).contains(&v))
            {
                return false;
            }
            properties::edit_styles(&mut state.document, &mut state.history, |styles| {
                let Some(style) = styles
                    .paragraphs
                    .iter_mut()
                    .find(|style| style.name == name)
                else {
                    return;
                };
                match id {
                    "design-prop-paragraph-stroke-miter" => style.stroke_miter_limit = value,
                    "design-prop-paragraph-stroke-width" => style.stroke_weight = value,
                    "design-prop-paragraph-stroke-tint" => {
                        style.stroke_tint = value.map(|v| v / 100.0);
                        if value.is_some() && stroke_tint_base.is_some() {
                            style.stroke = stroke_tint_base;
                        }
                    }
                    "design-prop-paragraph-fill-tint" => {
                        style.fill_tint = value.map(|v| v / 100.0);
                        if value.is_some() && tint_base.is_some() {
                            style.fill = tint_base;
                        }
                    }
                    "design-prop-size" => style.point_size = value,
                    "design-prop-auto-leading" => style.auto_leading = value,
                    "design-prop-tracking" => style.tracking = value,
                    "design-prop-baseline" => {
                        style.baseline_shift =
                            value.map(schist_layout::styles::BaselineShift::Offset)
                    }
                    "design-prop-before" => style.space_before = value,
                    "design-prop-after" => style.space_after = value,
                    "design-prop-left" => style.left_indent = value,
                    "design-prop-right" => style.right_indent = value,
                    "design-prop-first" => style.first_line_indent = value,
                    _ => {}
                }
            })
        }
        Target::Tab { .. } => false,
        Target::Character(name) => {
            let tint_base = state
                .document
                .styles
                .resolve_character(&name)
                .fill
                .filter(|ink| ink.tint.is_some())
                .map(|ink| ink.base_color().into_owned());
            let stroke_tint_base = state
                .document
                .styles
                .resolve_character(&name)
                .stroke
                .filter(|ink| ink.tint.is_some())
                .map(|ink| ink.base_color().into_owned());
            if matches!(id, "design-prop-char-size" | "design-prop-char-leading")
                && value.is_some_and(|value| value <= 0.0)
            {
                return false;
            }
            properties::edit_styles(&mut state.document, &mut state.history, |styles| {
                let Some(style) = styles
                    .characters
                    .iter_mut()
                    .find(|style| style.name == name)
                else {
                    return;
                };
                match id {
                    "design-prop-char-stroke-miter" => style.stroke_miter_limit = value,
                    "design-prop-char-stroke-width" => style.stroke_weight = value,
                    "design-prop-char-stroke-tint" => {
                        style.stroke_tint = value.map(|v| v / 100.0);
                        if value.is_some() && stroke_tint_base.is_some() {
                            style.stroke = stroke_tint_base;
                        }
                    }
                    "design-prop-char-fill-tint" => {
                        style.fill_tint = value.map(|v| v / 100.0);
                        if value.is_some() && tint_base.is_some() {
                            style.fill = tint_base;
                        }
                    }
                    "design-prop-char-size" => style.point_size = value,
                    "design-prop-char-tracking" => style.tracking = value,
                    "design-prop-char-baseline" => {
                        style.baseline_shift =
                            value.map(schist_layout::styles::BaselineShift::Offset)
                    }
                    _ => {}
                }
            })
        }
    }
}

/// Empty restores inheritance. Four-byte tags and explicit 0/1 values prevent
/// a malformed field from silently clearing other feature overrides.
fn parse_features(text: &str) -> Option<Vec<(String, bool)>> {
    let mut result = Vec::new();
    if text.trim().is_empty() {
        return Some(result);
    }
    for item in text.split(',') {
        let (tag, value) = item.split_once('=')?;
        let tag = tag.trim();
        if tag.len() != 4
            || !tag.bytes().all(|b| b.is_ascii_alphanumeric())
            || result.iter().any(|(previous, _)| previous == tag)
        {
            return None;
        }
        let enabled = match value.trim() {
            "1" => true,
            "0" => false,
            _ => return None,
        };
        result.push((tag.to_owned(), enabled));
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Some(result)
}

pub fn feature_text(features: &[(String, bool)]) -> String {
    features
        .iter()
        .map(|(tag, enabled)| format!("{tag}={}", u8::from(*enabled)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn all_text_frames(state: &DesignState) -> bool {
    !state.selection.is_empty()
        && state.selection.iter().all(|id| {
            state
                .document
                .object(*id)
                .is_some_and(|o| matches!(o.object, LayoutObject::TextFrame { .. }))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab_state() -> DesignState {
        let mut state = DesignState::new();
        state
            .document
            .styles
            .add_paragraph(schist_layout::ParagraphStyle {
                name: "Tab base".into(),
                list: schist_layout::lists::ListStyle {
                    tabs: Some(vec![schist_layout::lists::ListTab {
                        position: 72.12345,
                        alignment: "CharacterAlign".into(),
                        alignment_character: ",".into(),
                        leader: ". ".into(),
                    }]),
                    ..Default::default()
                },
                ..Default::default()
            });
        state
            .document
            .styles
            .add_paragraph(schist_layout::ParagraphStyle {
                name: "Tab child".into(),
                based_on: Some("Tab base".into()),
                ..Default::default()
            });
        state
    }

    #[test]
    fn tab_edits_capture_inherited_records_preserve_other_settings_and_undo_once() {
        for (id, value) in [
            ("design-prop-tab-position", "90.12345"),
            ("design-prop-tab-character", "€"),
            ("design-prop-tab-character", " "),
        ] {
            let mut state = tab_state();
            let before = state.document.clone();
            state.controls.field = tab_target(&state, "Tab child", 0);
            state.controls.paragraph = Some("Body".into());
            assert!(commit(&mut state, id, value));
            assert_eq!(state.history.undo_depth(), 1);
            let tab = &state
                .document
                .styles
                .paragraph("Tab child")
                .unwrap()
                .list
                .tabs
                .as_ref()
                .unwrap()[0];
            let original = &before
                .styles
                .paragraph("Tab base")
                .unwrap()
                .list
                .tabs
                .as_ref()
                .unwrap()[0];
            assert_eq!(tab.alignment, original.alignment);
            assert_eq!(tab.leader, original.leader);
            if id.ends_with("position") {
                assert_eq!(tab.alignment_character, original.alignment_character);
            } else {
                assert_eq!(tab.position, original.position);
            }
            assert_eq!(
                state.document.styles.paragraph("Tab base"),
                before.styles.paragraph("Tab base")
            );
            state.controls.field = tab_target(&state, "Tab child", 0);
            assert!(!commit(&mut state, id, value));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
    }

    #[test]
    fn tab_actions_are_single_edits_and_clearing_does_not_restore_inherited_stops() {
        for action in ["add", "remove", "LeftAlign", "RightAlign", "CenterAlign"] {
            let mut state = tab_state();
            let before = state.document.clone();
            let target = tab_target(&state, "Tab child", 0).unwrap();
            assert!(match action {
                "add" => add_tab(&mut state, &Target::Paragraph("Tab child".into())),
                "remove" => edit_tab(&mut state, &target, |tabs, index| {
                    tabs.remove(index);
                }),
                _ => set_tab_alignment(&mut state, &target, action),
            });
            let tabs = state
                .document
                .styles
                .resolve_paragraph("Tab child")
                .list
                .tabs
                .unwrap();
            if action == "remove" {
                assert!(tabs.is_empty());
            } else {
                assert_eq!(tabs[0].leader, ". ");
                assert_eq!(tabs[0].alignment_character, ",");
                assert_eq!(tabs[0].position, 72.12345);
            }
            assert_eq!(state.history.undo_depth(), 1);
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
        let mut state = tab_state();
        let target = Target::Paragraph("Tab child".into());
        assert!(edit_list(&mut state, &target, |list| list.tabs = Some(Vec::new())));
        let cleared = state.document.clone();
        assert!(edit_list(&mut state, &target, |list| list.tabs = None));
        assert!(tab_target(&state, "Tab child", 0).is_some());
        assert!(!edit_list(&mut state, &target, |list| list.tabs = None));
        assert_eq!(state.history.undo_depth(), 2);
        assert!(state.history.undo(&mut state.document));
        assert_eq!(state.document, cleared);
    }

    #[test]
    fn invalid_or_stale_tab_fields_never_modify_another_stop() {
        for (id, value) in [
            ("design-prop-tab-position", ""),
            ("design-prop-tab-position", "-1"),
            ("design-prop-tab-position", "NaN"),
            ("design-prop-tab-position", "inf"),
            ("design-prop-tab-character", ""),
            ("design-prop-tab-character", "ab"),
            ("design-prop-tab-character", "\t"),
            ("design-prop-tab-character", "\u{2028}"),
        ] {
            let mut state = tab_state();
            let before = state.document.clone();
            state.controls.field = tab_target(&state, "Tab child", 0);
            assert!(!commit(&mut state, id, value));
            assert_eq!(state.document, before);
            assert_eq!(state.history.undo_depth(), 0);
        }
        let mut state = tab_state();
        state.controls.field = tab_target(&state, "Tab child", 0);
        assert!(edit_list(
            &mut state,
            &Target::Paragraph("Tab base".into()),
            |list| list.tabs.as_mut().unwrap()[0].position = 150.0
        ));
        let before = state.document.clone();
        assert!(!commit(&mut state, "design-prop-tab-character", "."));
        assert_eq!(state.document, before);
        assert_eq!(state.history.undo_depth(), 1);
    }

    #[test]
    fn list_format_choices_preserve_native_names_and_change_the_captured_style_once() {
        use schist_layout::{list_numbering::CounterFormat, lists::NumberingFormat};
        for old in [
            None,
            Some(NumberingFormat::from(" 1, 2, 3, 4… ")),
            Some(NumberingFormat::from("Unknown native format")),
        ] {
            for choice in std::iter::once(None).chain(CounterFormat::ALL.into_iter().map(Some)) {
                let mut state = DesignState::new();
                state
                    .document
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == "Body")
                    .unwrap()
                    .list
                    .format = old.clone();
                state.controls.paragraph = Some("Different selection".into());
                let before = state.document.clone();
                let expected_change = match (&old, choice) {
                    (None, None) => false,
                    (Some(old), Some(new)) => old.counter_format() != Some(new),
                    _ => true,
                };
                let target = Target::Paragraph("Body".into());
                assert_eq!(
                    set_list_format(&mut state, &target, choice),
                    expected_change
                );
                assert_eq!(state.history.undo_depth(), usize::from(expected_change));
                assert!(!set_list_format(&mut state, &target, choice));
                if expected_change {
                    assert!(state.history.undo(&mut state.document));
                }
                assert_eq!(state.document, before);
            }
        }
    }

    #[test]
    fn level_expressions_and_restart_policies_target_captured_styles_and_undo_once() {
        use schist_layout::lists::RestartPolicy;
        for enabled in [None, Some(true), Some(false)] {
            let mut state = DesignState::new();
            let target = Target::Paragraph("Body".into());
            let body = state
                .document
                .styles
                .paragraphs
                .iter_mut()
                .find(|p| p.name == "Body")
                .unwrap();
            let policy = RestartPolicy {
                policy: "RangeOfLevels".into(),
                lower: 1,
                upper: 2,
            };
            body.list.restart_policy = Some(policy.clone());
            body.list.apply_restart_policy = Some(true);
            let before = state.document.clone();
            state.controls.paragraph = Some("Another selection".into());
            assert!(set_list_restart_policy(&mut state, &target, enabled));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(!set_list_restart_policy(&mut state, &target, enabled));
            if enabled == Some(false) {
                assert_eq!(
                    state
                        .document
                        .styles
                        .paragraph("Body")
                        .unwrap()
                        .list
                        .restart_policy,
                    Some(policy)
                );
            }
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
        for level in 2..=9 {
            let mut state = DesignState::new();
            state
                .document
                .styles
                .paragraphs
                .iter_mut()
                .find(|p| p.name == "Body")
                .unwrap()
                .list
                .level = Some(level);
            let expression = (1..level).map(|v| format!("^{v}.")).collect::<String>() + "^#^t";
            let before = state.document.clone();
            state.controls.field = Some(Target::Paragraph("Body".into()));
            assert!(commit(
                &mut state,
                "design-prop-list-expression",
                &expression
            ));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
    }

    #[test]
    fn list_fields_use_captured_styles_preserve_unchanged_tabs_and_undo_once() {
        use schist_layout::lists::{ListKind, ListTab};
        for (id, value) in [
            ("design-prop-list-start", "7"),
            ("design-prop-list-level", "9"),
            ("design-prop-list-bullet", "→"),
            ("design-prop-list-expression", "Item ^#)^t"),
            ("design-prop-list-tab", "24.125, 48"),
        ] {
            let mut state = DesignState::new();
            let before = state.document.clone();
            state.controls.field = Some(Target::Paragraph("Body".into()));
            state.controls.paragraph = Some("Different selection".into());
            assert!(commit(&mut state, id, value));
            assert_eq!(state.history.undo_depth(), 1);
            state.controls.field = Some(Target::Paragraph("Body".into()));
            assert!(!commit(&mut state, id, value));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
        let mut state = DesignState::new();
        let target = Target::Paragraph("Body".into());
        for kind in [
            None,
            Some(ListKind::None),
            Some(ListKind::Bullet),
            Some(ListKind::Numbered),
        ] {
            state
                .document
                .styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == "Body")
                .unwrap()
                .bullet = Some(schist_layout::styles::Bullet::Character {
                char: '•',
                indent: 8.0,
            });
            let before = state.document.clone();
            let depth = state.history.undo_depth();
            assert!(set_list_kind(&mut state, &target, kind));
            assert_eq!(state.history.undo_depth(), depth + 1);
            assert_eq!(
                state.document.styles.paragraph("Body").unwrap().bullet,
                None
            );
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
        for (id, value) in [
            ("design-prop-list-start", "0"),
            ("design-prop-list-level", "0"),
            ("design-prop-list-level", "10"),
            ("design-prop-list-level", "1.5"),
            ("design-prop-list-start", "1.5"),
            ("design-prop-list-start", "4294967296"),
            ("design-prop-list-bullet", "ab"),
            ("design-prop-list-tab", "NaN"),
            ("design-prop-list-tab", "1,-2"),
            ("design-prop-list-expression", "^1^t"),
        ] {
            let before = state.document.clone();
            state.controls.field = Some(target.clone());
            assert!(!commit(&mut state, id, value));
            assert_eq!(state.document, before);
        }
        state
            .document
            .styles
            .paragraphs
            .iter_mut()
            .find(|s| s.name == "Body")
            .unwrap()
            .list
            .tabs = Some(vec![
            ListTab {
                position: 12.3456,
                alignment: "CharacterAlign".into(),
                alignment_character: ",".into(),
                leader: ".".into(),
            },
            ListTab {
                position: 30.0,
                alignment: "RightAlign".into(),
                alignment_character: ".".into(),
                leader: String::new(),
            },
        ]);
        state
            .document
            .styles
            .paragraphs
            .iter_mut()
            .find(|s| s.name == "Body")
            .unwrap()
            .list
            .bullet = Some(schist_layout::lists::BulletSymbol {
            kind: "GlyphWithFont".into(),
            value: 65,
        });
        let before = state.document.clone();
        state.controls.field = Some(target.clone());
        let bullet = list_bullet_value(&state.document.styles.paragraph("Body").unwrap().list);
        assert!(!commit(&mut state, "design-prop-list-bullet", &bullet));
        assert_eq!(state.document, before);
        let value = list_tab_value(&state.document.styles.paragraph("Body").unwrap().list);
        state.controls.field = Some(target);
        assert!(!commit(&mut state, "design-prop-list-tab", &value));
        assert_eq!(state.document, before);
    }

    #[test]
    fn path_bracket_fields_keep_captured_targets_native_precision_and_one_undo() {
        let mut state = DesignState::new();
        let mut ids = Vec::new();
        for _ in 0..2 {
            let id = schist_layout::authoring::path_shape(
                &mut state.document,
                &mut state.history,
                0,
                schist_layout::ShapePath::ellipse(180.0, 70.0),
                schist_layout::authoring::Paint::none(),
            )
            .unwrap();
            schist_layout::text_path::attach(&mut state.document, &mut state.history, id).unwrap();
            ids.push(id);
        }
        state.history = Default::default();
        let before = state.document.clone();
        for input in ["NaN", "inf", "-1", "99999", "invalid"] {
            state.controls.field = Some(Target::Objects(ids.clone()));
            assert!(!commit(&mut state, "design-prop-path-start", input));
            assert_eq!(state.document, before);
        }
        state.controls.field = Some(Target::Objects(ids.clone()));
        state.selection.clear();
        assert!(commit(&mut state, "design-prop-path-start", "12.3456"));
        assert_eq!(state.history.undo_depth(), 1);
        for object in &state.document.objects {
            assert!(
                matches!(&object.object, LayoutObject::TextFrame { text_path: Some(path), .. } if path.start == 12.3456)
            );
        }
        state.controls.field = Some(Target::Objects(ids.clone()));
        assert!(!commit(&mut state, "design-prop-path-start", "12.35"));
        assert_eq!(state.history.undo_depth(), 1);
        assert!(state.history.undo(&mut state.document));
        assert_eq!(state.document, before);
        for input in ["100", ""] {
            state.controls.field = Some(Target::Objects(ids.clone()));
            assert!(commit(&mut state, "design-prop-path-end", input));
        }
        assert_eq!(state.document, before);
        assert_eq!(state.history.undo_depth(), 2);
        // Equal formatted values can still be a mixed selection. An explicit
        // entry must normalize them, while an unchanged single value keeps precision.
        for (object, value) in state.document.objects.iter_mut().zip([12.3456, 12.3499]) {
            let LayoutObject::TextFrame {
                text_path: Some(path),
                ..
            } = &mut object.object
            else {
                unreachable!()
            };
            path.start = value;
        }
        let mixed = state.document.clone();
        state.history = Default::default();
        state.controls.field = Some(Target::Objects(ids));
        assert!(commit(&mut state, "design-prop-path-start", "12.35"));
        assert_eq!(state.history.undo_depth(), 1);
        assert!(state.history.undo(&mut state.document));
        assert_eq!(state.document, mixed);
    }

    #[test]
    fn language_fields_preserve_imported_identity_and_commit_to_captured_targets_once() {
        for paragraph in [false, true] {
            let mut state = DesignState::new();
            state
                .document
                .styles
                .languages
                .push(schist_layout::language::LanguageResource {
                    id: "tr".into(),
                    name: "$ID/Romanian".into(),
                    spelling_vendor: Some("Vendor".into()),
                    ..Default::default()
                });
            let target = if paragraph {
                state.document.styles.paragraphs[0].language = Some("tr".into());
                Target::Paragraph(state.document.styles.paragraphs[0].name.clone())
            } else {
                state.document.styles.characters[0].language = Some("tr".into());
                Target::Character(state.document.styles.characters[0].name.clone())
            };
            let id = if paragraph {
                "design-prop-paragraph-language"
            } else {
                "design-prop-language"
            };
            let before = state.document.clone();
            for text in ["ro", "en-u", "en--US", "Language/unresolved"] {
                state.controls.field = Some(target.clone());
                assert!(!commit(&mut state, id, text));
                assert_eq!(state.document, before);
            }
            for text in ["tr", "TR_tr", "und", ""] {
                state.controls.field = Some(target.clone());
                state.controls.paragraph = Some("Selection changed".into());
                state.controls.character = Some("Selection changed".into());
                assert!(commit(&mut state, id, text));
                assert_eq!(
                    language_value(&state.document.styles, &target),
                    text.replace('_', "-").to_ascii_lowercase()
                );
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
                assert!(!state.history.undo(&mut state.document));
            }
        }
    }

    #[test]
    fn capitalization_uses_captured_targets_and_small_cap_preferences_undo_once() {
        use schist_text_engine::Capitalization::*;
        for paragraph in [false, true] {
            for value in [
                None,
                Some(Normal),
                Some(AllCaps),
                Some(SmallCaps),
                Some(OpenTypeAllSmallCaps),
            ] {
                let mut state = DesignState::new();
                let target = if paragraph {
                    state.document.styles.paragraphs[0].all_caps = Some(true);
                    Target::Paragraph(state.document.styles.paragraphs[0].name.clone())
                } else {
                    state.document.styles.characters[0].all_caps = Some(true);
                    Target::Character(state.document.styles.characters[0].name.clone())
                };
                let before = state.document.clone();
                assert!(set_capitalization(&mut state, &target, value));
                assert_eq!(state.history.undo_depth(), 1);
                let actual = if paragraph {
                    let s = &state.document.styles.paragraphs[0];
                    (s.all_caps, s.small_caps)
                } else {
                    let s = &state.document.styles.characters[0];
                    (s.all_caps, s.small_caps)
                };
                assert_eq!(
                    actual,
                    (value.map(|v| v.flags().0), value.map(|v| v.flags().1))
                );
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
            }
        }
        let mut state = DesignState::new();
        let before = state.document.clone();
        for value in ["", "NaN", "inf", "0", "201", "-1"] {
            state.controls.field = Some(Target::TextPreferences);
            assert!(!commit(&mut state, "design-prop-small-cap-size", value));
            assert_eq!(state.document, before);
        }
        for value in ["1", "55.5", "200"] {
            state.controls.field = Some(Target::TextPreferences);
            assert!(commit(&mut state, "design-prop-small-cap-size", value));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
    }

    #[test]
    fn directional_feature_choices_replace_only_their_tag_exceptions_and_undo_once() {
        for paragraph in [false, true] {
            for proportional in [false, true] {
                for value in [None, Some(false), Some(true)] {
                    let mut state = DesignState::new();
                    let target = if paragraph {
                        Target::Paragraph("Body".into())
                    } else {
                        Target::Character("Default".into())
                    };
                    let features = vec![
                        ("hkna".into(), true),
                        ("vkna".into(), false),
                        ("palt".into(), false),
                        ("vpal".into(), true),
                        ("liga".into(), true),
                    ];
                    if paragraph {
                        state
                            .document
                            .styles
                            .paragraphs
                            .iter_mut()
                            .find(|p| p.name == "Body")
                            .unwrap()
                            .features = features;
                    } else {
                        state
                            .document
                            .styles
                            .characters
                            .iter_mut()
                            .find(|p| p.name == "Default")
                            .unwrap()
                            .features = features;
                    }
                    let before = state.document.clone();
                    assert!(set_directional_feature(
                        &mut state,
                        &target,
                        proportional,
                        value
                    ));
                    assert_eq!(state.history.undo_depth(), 1);
                    let (mode, features) = if paragraph {
                        let style = state.document.styles.paragraph("Body").unwrap();
                        (style.directional_features, &style.features)
                    } else {
                        let style = state.document.styles.character("Default").unwrap();
                        (style.directional_features, &style.features)
                    };
                    assert_eq!(
                        if proportional {
                            mode.proportional_metrics
                        } else {
                            mode.kana
                        },
                        value
                    );
                    let removed = if proportional {
                        ["palt", "vpal"]
                    } else {
                        ["hkna", "vkna"]
                    };
                    assert!(features
                        .iter()
                        .all(|(tag, _)| !removed.contains(&tag.as_str())));
                    assert_eq!(features.len(), 3);
                    assert!(features.iter().any(|(tag, on)| tag == "liga" && *on));
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                }
            }
        }
    }

    #[test]
    fn text_join_and_miter_edits_keep_captured_targets_and_undo_one_gesture() {
        use schist_text_engine::TextStrokeJoin;
        for paragraph in [false, true] {
            let mut state = DesignState::new();
            let target = if paragraph {
                Target::Paragraph("Body".into())
            } else {
                Target::Character("Default".into())
            };
            let id = if paragraph {
                "design-prop-paragraph-stroke-miter"
            } else {
                "design-prop-char-stroke-miter"
            };
            let before = state.document.clone();
            for join in [
                TextStrokeJoin::Miter,
                TextStrokeJoin::Round,
                TextStrokeJoin::Bevel,
            ] {
                assert!(set_text_join(&mut state, &target, Some(join)));
                assert_eq!(state.history.undo_depth(), 1);
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
            }
            for invalid in ["-1", "NaN", "inf", "bad"] {
                state.controls.field = Some(target.clone());
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.history.undo_depth(), 0);
            }
            for value in ["0", "0.5", "4", "16"] {
                state.controls.field = Some(target.clone());
                state.controls.paragraph = Some("Missing".into());
                state.controls.character = Some("Missing".into());
                assert!(commit(&mut state, id, value));
                assert_eq!(state.history.undo_depth(), 1);
                let limit = match &target {
                    Target::Paragraph(name) => {
                        state
                            .document
                            .styles
                            .paragraph(name)
                            .unwrap()
                            .stroke_miter_limit
                    }
                    Target::Character(name) => {
                        state
                            .document
                            .styles
                            .character(name)
                            .unwrap()
                            .stroke_miter_limit
                    }
                    _ => unreachable!(),
                };
                assert_eq!(limit, Some(value.parse::<f32>().unwrap()));
                state.controls.field = Some(target.clone());
                assert!(commit(&mut state, id, ""));
                assert_eq!(state.document, before);
                assert!(state.history.undo(&mut state.document));
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
            }
        }
    }

    #[test]
    fn decoration_fields_distinguish_auto_inheritance_and_explicit_points_with_one_undo() {
        use schist_layout::decorations::{DecorationMeasure, DecorationPaint};
        for paragraph in [false, true] {
            for strike in [false, true] {
                let mut state = DesignState::new();
                let target = if paragraph {
                    Target::Paragraph("Body".into())
                } else {
                    Target::Character("Default".into())
                };
                let prefix = format!(
                    "design-prop-{}-{}-",
                    if paragraph { "para" } else { "char" },
                    if strike { "strike" } else { "underline" }
                );
                let before = state.document.clone();
                for field in ["weight", "offset", "tint"] {
                    let id = format!("{prefix}{field}");
                    for invalid in ["NaN", "inf", "bad", "-9999"] {
                        state.controls.field = Some(target.clone());
                        assert!(!commit(&mut state, &id, invalid));
                        assert_eq!(state.document, before);
                    }
                    for value in if field == "tint" {
                        vec!["0", "37.5", "100"]
                    } else {
                        vec!["0", "2.75", "Auto"]
                    } {
                        state.controls.field = Some(target.clone());
                        // A later panel choice cannot change this captured target.
                        state.controls.character = Some("Missing".into());
                        assert!(commit(&mut state, &id, value));
                        assert_eq!(state.history.undo_depth(), 1);
                        let style = match &target {
                            Target::Paragraph(name) => {
                                let s = state.document.styles.paragraph(name).unwrap();
                                if strike {
                                    &s.strike_style
                                } else {
                                    &s.underline_style
                                }
                            }
                            Target::Character(name) => {
                                let s = state.document.styles.character(name).unwrap();
                                if strike {
                                    &s.strike_style
                                } else {
                                    &s.underline_style
                                }
                            }
                            _ => unreachable!(),
                        };
                        if value == "Auto" {
                            assert_eq!(
                                if field == "weight" {
                                    style.weight
                                } else {
                                    style.offset
                                },
                                Some(DecorationMeasure::Auto)
                            );
                        }
                        assert!(state.history.undo(&mut state.document));
                        assert_eq!(state.document, before);
                    }
                    state.controls.field = Some(target.clone());
                    assert!(commit(&mut state, &id, "2"));
                    state.controls.field = Some(target.clone());
                    assert!(commit(&mut state, &id, ""));
                    assert_eq!(state.document, before);
                    assert!(state.history.undo(&mut state.document));
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                }
                for paint in [
                    DecorationPaint::Text,
                    DecorationPaint::None,
                    DecorationPaint::Ink(schist_layout::Ink::black()),
                ] {
                    assert!(edit_decoration(&mut state, &target, strike, |s| s.paint = Some(paint)));
                    assert_eq!(state.history.undo_depth(), 1);
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                }
                for value in [true, false] {
                    assert!(set_decoration_enabled(
                        &mut state,
                        &target,
                        strike,
                        Some(value)
                    ));
                    assert_eq!(state.history.undo_depth(), 1);
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                }
            }
        }
    }

    #[test]
    fn pattern_and_gap_controls_validate_capture_and_undo_each_property_once() {
        use schist_layout::decorations::DecorationPaint;
        for paragraph in [false, true] {
            for strike in [false, true] {
                let mut state = DesignState::new();
                let target = if paragraph {
                    Target::Paragraph("Body".into())
                } else {
                    Target::Character("Default".into())
                };
                let prefix = format!(
                    "design-prop-{}-{}-",
                    if paragraph { "para" } else { "char" },
                    if strike { "strike" } else { "underline" }
                );
                let before = state.document.clone();
                for (field, valid, invalid) in [
                    (
                        "stripes",
                        "0 25 75 100",
                        vec!["0", "25 20", "0 101", "0 NaN", "0 bad"],
                    ),
                    ("gap-tint", "37.5", vec!["-1", "101", "NaN", "inf", "bad"]),
                    (
                        "dots",
                        "5 7",
                        vec!["0", "0 0", "-1", "NaN", "inf", "1 2 3 4 5 6"],
                    ),
                    (
                        "dashes",
                        "6 3 2 1",
                        vec!["1", "0 0", "1 -1", "1 NaN", "1 1 1 1 1 1 1 1 1 1 1 1"],
                    ),
                ] {
                    let id = format!("{prefix}{field}");
                    for value in invalid {
                        state.controls.field = Some(target.clone());
                        assert!(!commit(&mut state, &id, value));
                        assert_eq!(state.document, before);
                    }
                    state.controls.field = Some(target.clone());
                    state.controls.character = Some("Missing".into());
                    assert!(commit(&mut state, &id, valid));
                    assert_eq!(state.history.undo_depth(), 1);
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                }
                let base = schist_layout::Ink::black();
                let named = base.named_tint("Quarter gap", 0.25).unwrap();
                assert!(edit_decoration(&mut state, &target, strike, |d| d
                    .gap_paint =
                    Some(DecorationPaint::Ink(named))));
                let named_doc = state.document.clone();
                state.controls.field = Some(target.clone());
                assert!(commit(&mut state, &format!("{prefix}gap-tint"), "70"));
                let resolved = match &target {
                    Target::Paragraph(name) => {
                        let s = state.document.styles.resolve_paragraph(name);
                        if strike {
                            s.strike_style
                        } else {
                            s.underline_style
                        }
                    }
                    Target::Character(name) => {
                        let s = state.document.styles.resolve_character(name);
                        if strike {
                            s.strike_style
                        } else {
                            s.underline_style
                        }
                    }
                    _ => unreachable!(),
                };
                assert_eq!(resolved.gap_paint, Some(DecorationPaint::Ink(base)));
                assert_eq!(resolved.gap_tint, Some(0.7));
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, named_doc);
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
            }
        }
    }

    #[test]
    fn fitting_controls_preserve_resource_names_capture_targets_and_undo_exactly_once() {
        use schist_layout::decorations::DecorationStroke;
        use schist_text_engine::{DecorationFit, TextDecorationPattern};
        for paragraph in [false, true] {
            for strike in [false, true] {
                for pattern in [
                    TextDecorationPattern::Dashes(vec![6.0, 3.0].into()),
                    TextDecorationPattern::Dots(vec![6.0]),
                ] {
                    for fitting in [
                        DecorationFit::Dashes,
                        DecorationFit::Gaps,
                        DecorationFit::DashesAndGaps,
                    ] {
                        let mut state = DesignState::new();
                        let target = if paragraph {
                            Target::Paragraph("Body".into())
                        } else {
                            Target::Character("Default".into())
                        };
                        edit_decoration(&mut state, &target, strike, |d| {
                            d.stroke = Some(DecorationStroke {
                                name: "Imported name".into(),
                                pattern: pattern.clone(),
                                fitting: DecorationFit::None,
                            })
                        });
                        let before = state.document.clone();
                        let depth = state.history.undo_depth();
                        state.controls.character = Some("Missing".into());
                        let supported = !(matches!(pattern, TextDecorationPattern::Dots(_))
                            && fitting == DecorationFit::Dashes);
                        assert_eq!(
                            set_decoration_fitting(&mut state, &target, strike, fitting),
                            supported
                        );
                        if !supported {
                            assert_eq!(state.document, before);
                            continue;
                        }
                        assert_eq!(state.history.undo_depth(), depth + 1);
                        let fitted = state.document.clone();
                        assert!(!set_decoration_fitting(
                            &mut state, &target, strike, fitting
                        ));
                        assert_eq!(state.history.undo_depth(), depth + 1);
                        let field = if matches!(pattern, TextDecorationPattern::Dots(_)) {
                            "dots"
                        } else {
                            "dashes"
                        };
                        let id = format!(
                            "design-prop-{}-{}-{field}",
                            if paragraph { "para" } else { "char" },
                            if strike { "strike" } else { "underline" }
                        );
                        state.controls.field = Some(target.clone());
                        assert!(commit(
                            &mut state,
                            &id,
                            if field == "dots" { "5 7" } else { "4 2" }
                        ));
                        let actual = if paragraph {
                            state.document.styles.resolve_paragraph("Body")
                        } else {
                            Default::default()
                        };
                        let stroke = if paragraph {
                            if strike {
                                actual.strike_style.stroke
                            } else {
                                actual.underline_style.stroke
                            }
                        } else {
                            let actual = state.document.styles.resolve_character("Default");
                            if strike {
                                actual.strike_style.stroke
                            } else {
                                actual.underline_style.stroke
                            }
                        }
                        .unwrap();
                        assert_eq!(stroke.name, "Imported name");
                        assert_eq!(stroke.fitting, fitting);
                        assert!(state.history.undo(&mut state.document));
                        assert_eq!(state.document, fitted);
                        assert!(state.history.undo(&mut state.document));
                        assert_eq!(state.document, before);
                    }
                }
            }
        }
    }

    #[test]
    fn dash_cap_controls_capture_targets_preserve_names_and_undo_once_without_length_edits_resetting_caps(
    ) {
        use schist_layout::decorations::DecorationStroke;
        use schist_text_engine::{DecorationCap, TextDecorationPattern};
        for paragraph in [false, true] {
            for strike in [false, true] {
                for cap in [DecorationCap::Round, DecorationCap::Projecting] {
                    let mut state = DesignState::new();
                    let target = if paragraph {
                        Target::Paragraph("Body".into())
                    } else {
                        Target::Character("Default".into())
                    };
                    edit_decoration(&mut state, &target, strike, |d| {
                        d.stroke = Some(DecorationStroke {
                            fitting: Default::default(),
                            name: "Imported dash".into(),
                            pattern: TextDecorationPattern::Dashes(vec![6.0, 3.0].into()),
                        })
                    });
                    state.history = Default::default();
                    let before = state.document.clone();
                    assert!(set_decoration_cap(&mut state, &target, strike, cap));
                    assert_eq!(state.history.undo_depth(), 1);
                    let capped = state.document.clone();
                    assert!(!set_decoration_cap(&mut state, &target, strike, cap));
                    let id = format!(
                        "design-prop-{}-{}-dashes",
                        if paragraph { "para" } else { "char" },
                        if strike { "strike" } else { "underline" }
                    );
                    state.controls.field = Some(target.clone());
                    assert!(!commit(&mut state, &id, "6 3"));
                    assert_eq!(state.document, capped);
                    state.controls.field = Some(target.clone());
                    assert!(commit(&mut state, &id, "5 2"));
                    assert!(state.document.all_decoration_strokes().iter().any(|s| s.name=="Imported dash" && matches!(&s.pattern,TextDecorationPattern::Dashes(d) if d.cap==cap && d.lengths==[5.0,2.0])));
                    assert_eq!(state.history.undo_depth(), 2);
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, capped);
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                }
            }
        }
    }

    #[test]
    fn unchanged_pattern_fields_preserve_native_names_solid_resets_and_undo_depth() {
        use schist_layout::decorations::DecorationStroke;
        use schist_text_engine::TextDecorationPattern;
        for paragraph in [false, true] {
            for strike in [false, true] {
                for (field, original, shown, changed_text, changed_pattern) in [
                    (
                        "stripes",
                        TextDecorationPattern::Stripes(vec![0.0, 25.0, 75.0, 100.0]),
                        "0 25 75 100",
                        "0 20 80 100",
                        TextDecorationPattern::Stripes(vec![0.0, 20.0, 80.0, 100.0]),
                    ),
                    (
                        "dashes",
                        TextDecorationPattern::Dashes(vec![6.0, 3.0].into()),
                        "6 3",
                        "6 3 2 1",
                        TextDecorationPattern::Dashes(vec![6.0, 3.0, 2.0, 1.0].into()),
                    ),
                    (
                        "dots",
                        TextDecorationPattern::Dots(vec![6.0]),
                        "6",
                        "5 7",
                        TextDecorationPattern::Dots(vec![5.0, 7.0]),
                    ),
                ] {
                    for stroke in [
                        None,
                        Some(DecorationStroke::solid()),
                        Some(DecorationStroke {
                            fitting: Default::default(),
                            name: "Imported stripe / native".into(),
                            pattern: TextDecorationPattern::Stripes(vec![0.0, 25.0, 75.0, 100.0]),
                        }),
                        Some(DecorationStroke {
                            fitting: Default::default(),
                            name: "Imported dash / native".into(),
                            pattern: TextDecorationPattern::Dashes(vec![6.0, 3.0].into()),
                        }),
                        Some(DecorationStroke {
                            fitting: Default::default(),
                            name: "Imported dots / native".into(),
                            pattern: TextDecorationPattern::Dots(vec![6.0]),
                        }),
                    ] {
                        let mut state = DesignState::new();
                        let target = if paragraph {
                            Target::Paragraph("Body".into())
                        } else {
                            Target::Character("Default".into())
                        };
                        edit_decoration(&mut state, &target, strike, |d| d.stroke = stroke.clone());
                        state.history = Default::default();
                        let id = format!(
                            "design-prop-{}-{}-{field}",
                            if paragraph { "para" } else { "char" },
                            if strike { "strike" } else { "underline" }
                        );
                        let matching = stroke.as_ref().is_some_and(|s| s.pattern == original);
                        let before = state.document.clone();
                        state.controls.field = Some(target.clone());
                        assert!(!commit(&mut state, &id, if matching { shown } else { "" }));
                        assert_eq!(state.document, before);
                        assert_eq!(state.history.undo_depth(), 0);
                        state.controls.field = Some(target.clone());
                        assert!(commit(&mut state, &id, changed_text));
                        assert_eq!(state.history.undo_depth(), 1);
                        if matching {
                            let changed = state.document.all_decoration_strokes();
                            assert!(changed
                                .iter()
                                .any(|s| s.name == stroke.as_ref().unwrap().name
                                    && s.pattern == changed_pattern));
                        }
                        assert!(state.history.undo(&mut state.document));
                        assert_eq!(state.document, before);
                    }
                }
            }
        }
    }

    #[test]
    fn text_stroke_controls_capture_targets_validate_values_and_undo_each_gesture_once() {
        for paragraph in [false, true] {
            let mut state = DesignState::new();
            let target = if paragraph {
                Target::Paragraph("Body".into())
            } else {
                Target::Character("Default".into())
            };
            let id = if paragraph {
                "design-prop-paragraph-stroke-width"
            } else {
                "design-prop-char-stroke-width"
            };
            let before = state.document.clone();
            state.controls.field = Some(target.clone());
            assert!(commit(&mut state, id, "2.5"));
            assert_eq!(state.history.undo_depth(), 1);
            for invalid in ["-1", "NaN", "inf", "bad"] {
                state.controls.field = Some(target.clone());
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.history.undo_depth(), 1);
            }
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
            for fill in [false, true] {
                for paint in [
                    schist_layout::Paint::None,
                    schist_layout::Paint::Ink(schist_layout::Ink::black()),
                ] {
                    assert!(set_text_paint(&mut state, &target, fill, Some(paint)));
                    assert_eq!(state.history.undo_depth(), 1);
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                }
            }
            for flag in ["fill", "stroke", "outside"] {
                for value in [false, true] {
                    assert!(set_text_paint_flag(&mut state, &target, flag, Some(value)));
                    assert_eq!(state.history.undo_depth(), 1);
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                }
            }
        }
    }

    #[test]
    fn object_style_fields_keep_captured_targets_validate_values_and_undo_once() {
        use schist_layout::{Ink, ObjectPaint, ObjectStyle, Paint};
        let mut state = DesignState::new();
        let base = Ink::cmyk("cyan", [1.0, 0.0, 0.0, 0.0]);
        state.document.styles.objects = vec![
            ObjectStyle {
                name: "Base".into(),
                enable_fill: Some(true),
                paint: ObjectPaint {
                    fill: Some(Paint::Ink(base.named_tint("Quarter", 0.25).unwrap())),
                    ..Default::default()
                },
                ..Default::default()
            },
            ObjectStyle {
                name: "Child".into(),
                based_on: Some("Base".into()),
                ..Default::default()
            },
        ];
        let before = state.document.clone();
        for value in ["NaN", "inf", "-1", "101", "bad"] {
            state.controls.field = Some(Target::ObjectStyle("Child".into()));
            assert!(!commit(&mut state, "design-prop-object-fill-tint", value));
            assert_eq!(state.document, before);
        }
        state.controls.field = Some(Target::ObjectStyle("Child".into()));
        state.controls.object_style = Some("Base".into());
        assert!(commit(&mut state, "design-prop-object-fill-tint", "70"));
        assert_eq!(state.history.undo_depth(), 1);
        let paint = state.document.styles.resolve_object("Child").paint;
        assert_eq!(paint.fill_ink(), Some(&base));
        assert_eq!(paint.fill_tint, Some(0.7));
        assert_eq!(state.document.styles.objects[0], before.styles.objects[0]);
        state.history.undo(&mut state.document);
        assert_eq!(state.document, before);
        for target in ["Child", "Base", "Missing"] {
            state.controls.field = Some(Target::ObjectStyle("Base".into()));
            assert!(!commit(&mut state, "design-prop-object-base", target));
            assert_eq!(state.document, before);
        }
    }
    #[test]
    fn position_and_document_preferences_undo_once_without_changing_explicit_offsets() {
        use schist_layout::styles::{BaselineShift, TextPosition};
        let mut state = DesignState::new();
        state.document.styles.characters[0].baseline_shift = Some(BaselineShift::Offset(8.0));
        let name = state.document.styles.characters[0].name.clone();
        let original = state.document.clone();
        for position in [
            TextPosition::Normal,
            TextPosition::Superscript,
            TextPosition::Subscript,
        ] {
            assert!(set_position(
                &mut state,
                &Target::Character(name.clone()),
                Some(position)
            ));
            assert_eq!(state.history.undo_depth(), 1);
            assert_eq!(
                state
                    .document
                    .styles
                    .character(&name)
                    .unwrap()
                    .baseline_shift,
                Some(BaselineShift::Offset(8.0))
            );
            assert_eq!(
                state.document.styles.character(&name).unwrap().position,
                Some(position)
            );
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, original);
        }
        for (id, value) in [
            ("design-prop-superscript-size", "65"),
            ("design-prop-subscript-size", "75"),
            ("design-prop-superscript-position", "-45"),
            ("design-prop-subscript-position", "25"),
        ] {
            for invalid in ["NaN", "inf", "501", "-501", ""] {
                state.controls.field = Some(Target::TextPreferences);
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.document, original);
            }
            state.controls.field = Some(Target::TextPreferences);
            assert!(commit(&mut state, id, value));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, original);
        }
    }

    #[test]
    fn swatch_fields_edit_captured_definitions_keep_cmyk_and_reject_invalid_values() {
        let mut state = DesignState::new();
        let base = schist_layout::Ink::cmyk("Native", [0.6, 0.4, 0.2, 0.1]);
        let tint = base.named_tint("Quarter", 0.25).unwrap();
        state.document.inks.extend([base.clone(), tint.clone()]);
        let original = state.document.clone();
        for (id, ink, value) in [
            ("design-prop-swatch-tint", &tint, "50"),
            ("design-prop-swatch-cmyk-c", &base, "80"),
        ] {
            for invalid in ["NaN", "inf", "-1", "101", ""] {
                state.controls.field = Some(Target::Swatch(ink.clone()));
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.document, original);
            }
            state.controls.field = Some(Target::Swatch(ink.clone()));
            state.controls.swatch = Some(0);
            assert!(commit(&mut state, id, value));
            assert_eq!(state.history.undo_depth(), 1);
            let changed = state.document.ink(&ink.name).unwrap();
            if ink.tint.is_some() {
                assert_eq!(changed.tint_amount(), 0.5);
            } else {
                assert_eq!(changed.source_cmyk, Some([0.8, 0.4, 0.2, 0.1]));
            }
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, original);
        }
    }
    #[test]
    fn baseline_fields_commit_once_restore_inheritance_and_reject_nonfinite_values() {
        for (id, target) in [
            ("design-prop-baseline", Target::Paragraph("Body".into())),
            (
                "design-prop-char-baseline",
                Target::Character("Default".into()),
            ),
        ] {
            let mut state = DesignState::new();
            let original = state.document.clone();
            for invalid in ["NaN", "inf", "-inf", "garbage"] {
                state.controls.field = Some(target.clone());
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.document, original);
                assert_eq!(state.history.undo_depth(), 0);
            }
            for value in ["-12.5", "0", "8.25"] {
                state.controls.field = Some(target.clone());
                state.controls.paragraph = Some("Other".into());
                state.controls.character = Some("Other".into());
                assert!(commit(&mut state, id, value));
                assert_eq!(state.history.undo_depth(), 1);
                let actual = match &target {
                    Target::Paragraph(name) => {
                        state
                            .document
                            .styles
                            .paragraph(name)
                            .unwrap()
                            .baseline_shift
                    }
                    Target::Character(name) => {
                        state
                            .document
                            .styles
                            .character(name)
                            .unwrap()
                            .baseline_shift
                    }
                    _ => unreachable!(),
                };
                assert_eq!(
                    actual,
                    Some(schist_layout::styles::BaselineShift::Offset(
                        value.parse().unwrap()
                    ))
                );
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, original);
                assert!(state.history.redo(&mut state.document));
                state.controls.field = Some(target.clone());
                assert!(commit(&mut state, id, ""));
                assert_eq!(state.document, original);
                state.history.clear();
            }
        }
    }

    #[test]
    fn tint_fields_commit_once_to_the_captured_targets_and_blank_styles_inherit() {
        for paragraph in [false, true] {
            let mut state = DesignState::new();
            let (id, name, target) = if paragraph {
                (
                    "design-prop-paragraph-fill-tint",
                    "Body",
                    Target::Paragraph("Body".into()),
                )
            } else {
                (
                    "design-prop-char-fill-tint",
                    "Default",
                    Target::Character("Default".into()),
                )
            };
            let before = state.document.clone();
            for invalid in ["-1", "100.01", "NaN", "inf"] {
                state.controls.field = Some(target.clone());
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.document, before);
            }
            state.controls.field = Some(target.clone());
            assert!(commit(&mut state, id, "12.5"));
            assert_eq!(state.history.undo_depth(), 1);
            let value = if paragraph {
                state.document.styles.paragraph(name).unwrap().fill_tint
            } else {
                state.document.styles.character(name).unwrap().fill_tint
            };
            assert_eq!(value, Some(0.125));
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
            assert!(state.history.redo(&mut state.document));
            state.controls.field = Some(target);
            assert!(commit(&mut state, id, ""));
            assert_eq!(state.document, before);
        }
        for id in ["design-prop-fill-tint", "design-prop-stroke-tint"] {
            let mut state = DesignState::new();
            let objects: Vec<_> = (0..3)
                .map(|_| {
                    schist_layout::authoring::rectangle(
                        &mut state.document,
                        &mut schist_layout::History::default(),
                        0,
                        schist_layout::Rect::new(5.0, 5.0, 10.0, 10.0),
                        schist_layout::authoring::Paint::filled("Black"),
                    )
                    .unwrap()
                })
                .collect();
            let before = state.document.clone();
            state.controls.field = Some(Target::Objects(objects));
            state.selection.clear();
            assert!(commit(&mut state, id, "25"));
            assert!(state
                .document
                .objects
                .iter()
                .all(|o| object_property(id).unwrap().value(o) == Some(25.0)));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
    }

    #[test]
    fn section_fields_edit_the_captured_boundary_once_and_reject_invalid_starts() {
        for field in [
            "design-prop-number-start",
            "design-prop-number-prefix",
            "design-prop-section-name",
            "design-prop-section-marker",
        ] {
            let mut state = DesignState::new();
            for _ in 0..4 {
                state.document.add_page(schist_layout::Page::a4());
            }
            state.document.pages[2].section = Some(schist_layout::Section::default());
            let before = state.document.clone();
            state.controls.field = Some(Target::Section(2));
            state.page = Some(4);
            assert!(commit(&mut state, field, "37"));
            assert_eq!(state.history.undo_depth(), 1);
            assert_eq!(state.document.pages[0], before.pages[0]);
            assert!(state.document.pages[4].section.is_none());
            if field == "design-prop-number-start" {
                assert_eq!(state.document.page_number(4), "39");
            }
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
        let mut state = DesignState::new();
        let before = state.document.clone();
        for text in ["", "NaN", "inf", "-1", "0", "1.5", "1000000", "garbage"] {
            state.controls.field = Some(Target::Section(0));
            assert!(!commit(&mut state, "design-prop-number-start", text));
        }
        assert_eq!(state.document, before);
        assert_eq!(state.history.undo_depth(), 0);
    }

    #[test]
    fn committing_targets_the_original_selection_once() {
        for count in 1..12 {
            let mut state = DesignState::new();
            let ids: Vec<_> = (0..count)
                .map(|_| {
                    schist_layout::authoring::rectangle(
                        &mut state.document,
                        &mut state.history,
                        0,
                        schist_layout::Rect::new(0.0, 0.0, 20.0, 10.0),
                        schist_layout::authoring::Paint::none(),
                    )
                    .unwrap()
                })
                .collect();
            state.history.clear();
            let before = state.document.clone();
            state.controls.field = Some(Target::Objects(ids.clone()));
            state.selection.clear();
            assert!(commit(&mut state, "design-prop-width", "45"));
            assert!(ids
                .iter()
                .all(|id| state.document.object(*id).unwrap().bounds.width == 45.0));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(!commit(&mut state, "design-prop-width", "46"));
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
    }
    #[test]
    fn empty_style_numbers_restore_inheritance_and_invalid_numbers_do_nothing() {
        let mut state = DesignState::new();
        let name = state.document.styles.paragraphs[0].name.clone();
        for text in ["NaN", "inf", "-1", "0", "garbage"] {
            let before = state.document.clone();
            state.controls.field = Some(Target::Paragraph(name.clone()));
            assert!(!commit(&mut state, "design-prop-size", text));
            assert_eq!(state.document, before);
        }
        state.controls.field = Some(Target::Paragraph(name.clone()));
        commit(&mut state, "design-prop-size", "20");
        state.controls.field = Some(Target::Paragraph(name.clone()));
        assert!(commit(&mut state, "design-prop-size", ""));
        assert_eq!(
            state.document.styles.paragraph(&name).unwrap().point_size,
            None
        );
    }

    #[test]
    fn named_font_fields_capture_the_target_and_restore_inheritance_in_one_edit() {
        for paragraph in [false, true] {
            let name = if paragraph { "Body" } else { "Default" };
            let target = if paragraph {
                Target::Paragraph(name.into())
            } else {
                Target::Character(name.into())
            };
            let id = if paragraph {
                "design-prop-paragraph-font-style"
            } else {
                "design-prop-font-style"
            };
            for face in ["Light", "Regular", "Bold Condensed", "書体 W3 & Narrow"] {
                let mut state = DesignState::new();
                let before = state.document.clone();
                state.controls.field = Some(target.clone());
                state.controls.character = Some("Bold".into());
                state.controls.paragraph = Some("Default".into());
                assert!(commit(&mut state, id, face));
                assert_eq!(state.history.undo_depth(), 1);
                let selected = if paragraph {
                    &state.document.styles.paragraph(name).unwrap().font_style
                } else {
                    &state.document.styles.character(name).unwrap().font_style
                };
                assert_eq!(selected.as_deref(), Some(face));
                let after = state.document.clone();
                state.controls.field = Some(target.clone());
                assert!(!commit(&mut state, id, "Bad\nFace"));
                assert_eq!(state.document, after);
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
                assert!(state.history.redo(&mut state.document));
                assert_eq!(state.document, after);
                state.controls.field = Some(target.clone());
                assert!(commit(&mut state, id, ""));
                assert_eq!(state.document, before);
            }
        }
    }

    #[test]
    fn an_unchanged_font_field_does_not_flatten_legacy_face_inheritance() {
        for paragraph in [false, true] {
            for bold in [None, Some(false), Some(true)] {
                for italic in [None, Some(false), Some(true)] {
                    if bold.is_none() && italic.is_none() {
                        continue;
                    }
                    let mut state = DesignState::new();
                    let target = if paragraph {
                        let style = state
                            .document
                            .styles
                            .paragraphs
                            .iter_mut()
                            .find(|s| s.name == "Body")
                            .unwrap();
                        style.bold = bold;
                        style.italic = italic;
                        Target::Paragraph("Body".into())
                    } else {
                        let style = state
                            .document
                            .styles
                            .characters
                            .iter_mut()
                            .find(|s| s.name == "Default")
                            .unwrap();
                        style.bold = bold;
                        style.italic = italic;
                        Target::Character("Default".into())
                    };
                    let shown = font_style_value(&state.document.styles, &target);
                    assert!(!shown.is_empty());
                    let before = state.document.clone();
                    state.controls.field = Some(target.clone());
                    assert!(!commit(&mut state, "design-prop-font-style", &shown));
                    assert_eq!(state.document, before);
                    assert_eq!(state.history.undo_depth(), 0);
                    state.controls.field = Some(target.clone());
                    assert!(commit(&mut state, "design-prop-font-style", ""));
                    assert!(font_style_value(&state.document.styles, &target).is_empty());
                    assert_eq!(state.history.undo_depth(), 1);
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                }
            }
        }
    }

    #[test]
    fn leading_fields_distinguish_auto_zero_and_inheritance_with_one_step_undo() {
        use schist_layout::styles::Leading;
        for paragraph in [false, true] {
            let name = if paragraph { "Body" } else { "Default" };
            let target = if paragraph {
                Target::Paragraph(name.into())
            } else {
                Target::Character(name.into())
            };
            let id = if paragraph {
                "design-prop-leading"
            } else {
                "design-prop-char-leading"
            };
            for (text, expected) in [
                ("Auto", Some(Leading::Auto)),
                ("0", Some(Leading::Points(0.0))),
                ("19.5", Some(Leading::Points(19.5))),
            ] {
                let mut state = DesignState::new();
                let before = state.document.clone();
                state.controls.field = Some(target.clone());
                assert!(commit(&mut state, id, text));
                assert_eq!(state.history.undo_depth(), 1);
                let actual = if paragraph {
                    state.document.styles.paragraph(name).unwrap().leading
                } else {
                    state.document.styles.character(name).unwrap().leading
                };
                assert_eq!(actual, expected);
                let after = state.document.clone();
                for invalid in ["-1", "NaN", "inf", "19 pt", "automatic"] {
                    state.controls.field = Some(target.clone());
                    assert!(!commit(&mut state, id, invalid));
                    assert_eq!(state.document, after);
                    assert_eq!(state.history.undo_depth(), 1);
                }
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
                assert!(state.history.redo(&mut state.document));
                assert_eq!(state.document, after);
                state.controls.field = Some(target.clone());
                assert!(commit(&mut state, id, ""));
                let cleared = if paragraph {
                    state.document.styles.paragraph(name).unwrap().leading
                } else {
                    state.document.styles.character(name).unwrap().leading
                };
                assert_eq!(cleared, None);
            }
        }
        for percent in ["0", "150", "500"] {
            let mut state = DesignState::new();
            let before = state.document.clone();
            state.controls.field = Some(Target::Paragraph("Body".into()));
            assert!(commit(&mut state, "design-prop-auto-leading", percent));
            assert_eq!(state.history.undo_depth(), 1);
            for invalid in ["-1", "501", "NaN"] {
                state.controls.field = Some(Target::Paragraph("Body".into()));
                assert!(!commit(&mut state, "design-prop-auto-leading", invalid));
            }
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
    }

    #[test]
    fn feature_fields_commit_once_to_the_captured_style_and_reject_partial_invalid_input() {
        for paragraph in [false, true] {
            let mut state = DesignState::new();
            let name = if paragraph { "Body" } else { "Default" };
            let target = if paragraph {
                Target::Paragraph(name.into())
            } else {
                Target::Character(name.into())
            };
            let id = if paragraph {
                "design-prop-paragraph-features"
            } else {
                "design-prop-char-features"
            };
            let before = state.document.clone();
            state.controls.field = Some(target.clone());
            assert!(commit(&mut state, id, "liga=1, kern=0, ss03=1"));
            assert_eq!(state.history.undo_depth(), 1);
            let features = if paragraph {
                &state.document.styles.paragraph(name).unwrap().features
            } else {
                &state.document.styles.character(name).unwrap().features
            };
            assert_eq!(feature_text(features), "kern=0, liga=1, ss03=1");
            let after = state.document.clone();
            for invalid in [
                "liga",
                "liga=2",
                "liga=1,",
                "liga=0, liga=1",
                "abc=1",
                "éabc=1",
                "liga=1, kern=no",
            ] {
                state.controls.field = Some(target.clone());
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.document, after);
                assert_eq!(state.history.undo_depth(), 1);
            }
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
            assert!(state.history.redo(&mut state.document));
            assert_eq!(state.document, after);
            state.controls.field = Some(target);
            assert!(commit(&mut state, id, ""));
            assert_eq!(state.document, before);
        }
    }
}
