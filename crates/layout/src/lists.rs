//! Automatic paragraph markers are generated content, never story bytes.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListKind {
    None,
    Bullet,
    Numbered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkerAlignment {
    Left,
    Center,
    Right,
}

/// Native bullet definitions can be Unicode scalars or font glyph indices.
/// Keeping the type prevents a glyph ID from being mistaken for a character.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BulletSymbol {
    pub kind: String,
    pub value: u32,
}
impl BulletSymbol {
    pub fn unicode(value: char) -> Self {
        Self {
            kind: "UnicodeOnly".into(),
            value: u32::from(value),
        }
    }

    /// The bullet character. One remembered with its font (UnicodeWithFont)
    /// is still that character, set in the paragraph's font; a glyph id of a
    /// font (GlyphWithFont) is not a character.
    pub fn character(&self) -> Option<char> {
        matches!(self.kind.as_str(), "UnicodeOnly" | "UnicodeWithFont")
            .then(|| char::from_u32(self.value))
            .flatten()
            .filter(|c| !c.is_control())
    }
}

/// Named numbering sequences are independent of paragraph-style identity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NumberingList {
    pub id: String,
    pub name: String,
    pub across_stories: bool,
    pub across_documents: bool,
    pub labels: Vec<(String, String)>,
}

/// Native paragraph tab records, shared by source tabs and list-marker spacing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListTab {
    pub position: f32,
    pub alignment: String,
    pub alignment_character: String,
    pub leader: String,
}

impl ListTab {
    /// Supported source-tab anchors. Retain unknown native strings in the
    /// record and diagnose them instead of silently normalizing imported data.
    pub fn text_alignment(&self) -> Option<schist_text_engine::TabAlignment> {
        use schist_text_engine::TabAlignment;
        Some(match self.alignment.as_str() {
            "LeftAlign" => TabAlignment::Leading,
            "RightAlign" => TabAlignment::Trailing,
            "CenterAlign" => TabAlignment::Center,
            "CharacterAlign" => {
                let mut chars = self.alignment_character.chars();
                let character = chars
                    .next()
                    .filter(|c| !c.is_control() && !matches!(c, '\u{2028}' | '\u{2029}'))?;
                if chars.next().is_some() {
                    return None;
                }
                TabAlignment::Character(character)
            }
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartPolicy {
    pub policy: String,
    pub lower: u32,
    pub upper: u32,
}

/// A native numbering format is either a named string or an enumeration.
/// They inherit atomically, so an override cannot accidentally retain its
/// parent's XML type. Existing serialized strings remain named formats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NumberingFormat {
    Named(String),
    Enumeration { enumeration: String },
}
impl NumberingFormat {
    pub fn value(&self) -> &str {
        match self {
            Self::Named(value) => value,
            Self::Enumeration { enumeration } => enumeration,
        }
    }
}
impl From<&str> for NumberingFormat {
    fn from(value: &str) -> Self {
        Self::Named(value.into())
    }
}
impl From<String> for NumberingFormat {
    fn from(value: String) -> Self {
        Self::Named(value)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ListStyle {
    pub kind: Option<ListKind>,
    pub bullet: Option<BulletSymbol>,
    pub start: Option<u32>,
    pub continue_numbering: Option<bool>,
    pub level: Option<u32>,
    pub apply_restart_policy: Option<bool>,
    pub restart_policy: Option<RestartPolicy>,
    pub tabs: Option<Vec<ListTab>>,
    pub list: Option<String>,
    pub format: Option<NumberingFormat>,
    pub expression: Option<String>,
    pub text_after: Option<String>,
    pub bullet_character_style: Option<String>,
    pub numbering_character_style: Option<String>,
    pub bullet_font: Option<String>,
    pub bullet_font_style: Option<String>,
    pub bullet_alignment: Option<MarkerAlignment>,
    pub numbering_alignment: Option<MarkerAlignment>,
    /// Legacy Schist literal-bullet gap in points. Native lists use their
    /// text-after expression and paragraph tab stops instead.
    pub legacy_gap: Option<f32>,
}

impl ListStyle {
    pub fn active(&self) -> bool {
        matches!(self.kind, Some(ListKind::Bullet | ListKind::Numbered))
    }
    pub fn over(&self, base: &Self) -> Self {
        fn first<T: Clone>(value: &Option<T>, base: &Option<T>) -> Option<T> {
            value.as_ref().or(base.as_ref()).cloned()
        }
        macro_rules! inherited {
            ($($field:ident),+ $(,)?) => {
                Self { $($field: first(&self.$field, &base.$field)),+ }
            };
        }
        inherited!(
            kind,
            bullet,
            start,
            continue_numbering,
            level,
            apply_restart_policy,
            restart_policy,
            tabs,
            list,
            format,
            expression,
            text_after,
            bullet_character_style,
            numbering_character_style,
            bullet_font,
            bullet_font_style,
            bullet_alignment,
            numbering_alignment,
            legacy_gap
        )
    }

    pub fn from_legacy(bullet: Option<crate::styles::Bullet>) -> Self {
        use crate::styles::Bullet;
        match bullet {
            None => Self::default(),
            Some(Bullet::None) => Self {
                kind: Some(ListKind::None),
                ..Self::default()
            },
            Some(Bullet::Character { char, indent }) => Self {
                kind: Some(ListKind::Bullet),
                bullet: Some(BulletSymbol::unicode(char)),
                text_after: Some(String::new()),
                legacy_gap: Some(indent),
                ..Self::default()
            },
            Some(Bullet::Numbered { start, suffix }) => Self {
                kind: Some(ListKind::Numbered),
                start: Some(u32::try_from(start).unwrap_or(1).max(1)),
                expression: Some(format!("^#{}^t", suffix.to_string().replace('^', "^^"))),
                ..Self::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParagraphStyle, StyleSet};

    #[test]
    fn numbering_format_value_and_type_inherit_together_and_load_legacy_strings() {
        let named: NumberingFormat = serde_json::from_str("\"Arabic\"").unwrap();
        let enumerated = NumberingFormat::Enumeration {
            enumeration: "Arabic".into(),
        };
        assert_eq!(named, NumberingFormat::Named("Arabic".into()));
        for base in [&named, &enumerated] {
            let base = ListStyle {
                format: Some(base.clone()),
                ..Default::default()
            };
            for value in [None, Some(named.clone()), Some(enumerated.clone())] {
                let child = ListStyle {
                    format: value.clone(),
                    ..Default::default()
                };
                let inherited = child.over(&base);
                assert_eq!(inherited.format, value.or_else(|| base.format.clone()));
                let encoded = serde_json::to_string(&inherited).unwrap();
                assert_eq!(
                    serde_json::from_str::<ListStyle>(&encoded).unwrap(),
                    inherited
                );
            }
        }
    }

    #[test]
    fn list_properties_inherit_independently_with_explicit_resets_and_legacy_precedence() {
        let mut styles = StyleSet::default();
        styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            list: ListStyle {
                kind: Some(ListKind::Numbered),
                start: Some(7),
                continue_numbering: Some(true),
                expression: Some("^#)^t".into()),
                list: Some("opaque".into()),
                bullet: Some(BulletSymbol::unicode('●')),
                ..Default::default()
            },
            ..Default::default()
        });
        styles.add_paragraph(ParagraphStyle {
            name: "Child".into(),
            based_on: Some("Base".into()),
            list: ListStyle {
                start: Some(2),
                continue_numbering: Some(false),
                ..Default::default()
            },
            ..Default::default()
        });
        let resolved = styles.resolve_paragraph("Child").list;
        assert_eq!(resolved.kind, Some(ListKind::Numbered));
        assert_eq!(resolved.start, Some(2));
        assert_eq!(resolved.continue_numbering, Some(false));
        assert_eq!(resolved.expression.as_deref(), Some("^#)^t"));
        assert_eq!(resolved.list.as_deref(), Some("opaque"));
        for legacy in [
            crate::styles::Bullet::None,
            crate::styles::Bullet::Character {
                char: '•',
                indent: 8.0,
            },
        ] {
            styles.paragraphs[1].bullet = Some(legacy);
            let resolved = styles.resolve_paragraph("Child").list;
            assert_eq!(resolved.kind, ListStyle::from_legacy(Some(legacy)).kind);
            styles.paragraphs[1].list.kind = Some(ListKind::Numbered);
            assert_eq!(
                styles.resolve_paragraph("Child").list.kind,
                Some(ListKind::Numbered)
            );
            styles.paragraphs[1].list.kind = None;
        }
        let encoded = serde_json::to_string(&styles).unwrap();
        assert_eq!(serde_json::from_str::<StyleSet>(&encoded).unwrap(), styles);
        assert!(BulletSymbol {
            kind: "GlyphWithFont".into(),
            value: 65
        }
        .character()
        .is_none());
        assert!(BulletSymbol {
            kind: "UnicodeOnly".into(),
            value: 0xd800
        }
        .character()
        .is_none());
    }
}
