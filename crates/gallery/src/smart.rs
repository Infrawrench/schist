//! Smart albums: saved searches over what the gallery already indexes.
//!
//! A smart album is a bucket with a [`RuleGroup`] on it. The rules read
//! only metadata the gallery keeps anyway — the culling decisions, the
//! XMP keywords, the capture time, the camera and lens from the EXIF
//! pass, the position and nearest city, the People tags, the folder,
//! whether there is an edit and whether earlier edits were kept — so
//! evaluating one is a walk over facts in memory, never a decode. The
//! bucket machinery does the rest: the same background pass that keeps
//! the query and area buckets current re-evaluates the rules whenever
//! the index, the decisions or the tags move.
//!
//! The rules persist as JSON inside the bucket's `library.json` entry,
//! under `filter`; a library written before smart albums simply has
//! none.

use crate::culling::{ColourLabel, CullFlag, PhotoCulling};
use std::path::{Path, PathBuf};

/// How a group combines its rules.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Match {
    /// Every rule holds.
    #[default]
    All,
    /// At least one rule holds.
    Any,
    /// No rule holds — the group excludes what its rules describe.
    None,
}

impl Match {
    pub const ALL: [Match; 3] = [Match::All, Match::Any, Match::None];
}

/// How a rating rule compares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compare {
    #[default]
    AtLeast,
    AtMost,
    Exactly,
}

impl Compare {
    pub const ALL: [Compare; 3] = [Compare::AtLeast, Compare::AtMost, Compare::Exactly];
}

/// A family of file formats, by extension. Coarse on purpose: "raw"
/// is a question people ask, "Olympus ORF" rarely is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    #[default]
    Jpeg,
    Png,
    Heif,
    Avif,
    JpegXl,
    Webp,
    Tiff,
    Raw,
    /// Layered documents: PSD/PSB, Affinity, Krita, OpenRaster, GIMP.
    Layered,
    Video,
    /// Anything the other kinds do not name.
    Other,
}

impl FileKind {
    pub const ALL: [FileKind; 11] = [
        FileKind::Jpeg,
        FileKind::Png,
        FileKind::Heif,
        FileKind::Avif,
        FileKind::JpegXl,
        FileKind::Webp,
        FileKind::Tiff,
        FileKind::Raw,
        FileKind::Layered,
        FileKind::Video,
        FileKind::Other,
    ];

    /// The kind a file belongs to, from its extension.
    pub fn of(path: &Path) -> FileKind {
        if crate::is_video(path) {
            return FileKind::Video;
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "jpg" | "jpeg" | "jpe" | "jfif" => FileKind::Jpeg,
            "png" => FileKind::Png,
            "heic" | "heif" | "hif" => FileKind::Heif,
            "avif" => FileKind::Avif,
            "jxl" => FileKind::JpegXl,
            "webp" => FileKind::Webp,
            "tif" | "tiff" => FileKind::Tiff,
            "psd" | "psb" | "afphoto" | "af" | "afdesign" | "kra" | "ora" | "xcf" => {
                FileKind::Layered
            }
            e if RAW_EXTENSIONS.contains(&e) => FileKind::Raw,
            _ => FileKind::Other,
        }
    }
}

/// Camera raw extensions, lowercase. DNG counts: it is what a phone's
/// "RAW" switch writes.
const RAW_EXTENSIONS: &[&str] = &[
    "dng", "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "rwl",
    "pef", "srw", "x3f", "3fr", "fff", "iiq", "erf", "mef", "mos", "mrw", "kdc", "dcr", "k25",
];

fn yes() -> bool {
    true
}

/// One condition on a photo. Text comparisons ignore case and match
/// anywhere in the value ("canon" finds "Canon EOS R5"); a rule whose
/// text is blank says nothing and is left out of its group, so a
/// half-filled editor never empties an album.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Rule {
    Rating {
        #[serde(default)]
        compare: Compare,
        stars: u8,
    },
    Flag {
        flag: CullFlag,
    },
    Label {
        label: ColourLabel,
    },
    Keyword {
        text: String,
    },
    /// Capture date, inclusive at both ends, as `YYYY-MM-DD` (a year
    /// `YYYY` or month `YYYY-MM` works too: it is compared as a prefix
    /// range). Either end may be left open.
    Taken {
        #[serde(default)]
        from: String,
        #[serde(default)]
        to: String,
    },
    Camera {
        text: String,
    },
    Lens {
        text: String,
    },
    FileType {
        file: FileKind,
    },
    HasLocation {
        #[serde(default = "yes")]
        yes: bool,
    },
    /// The nearest gazetteer city the photo groups under.
    Place {
        text: String,
    },
    /// A named person from People, by name.
    Person {
        name: String,
    },
    /// Inside this folder, at any depth.
    Folder {
        path: PathBuf,
    },
    Edited {
        #[serde(default = "yes")]
        yes: bool,
    },
    /// Earlier saves of the edit were kept under `versions/`.
    HasVersions {
        #[serde(default = "yes")]
        yes: bool,
    },
    Group(RuleGroup),
}

/// The kinds of rule, for the editor's menu and for asking whether a
/// group needs an expensive fact gathered at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleKind {
    Rating,
    Flag,
    Label,
    Keyword,
    Taken,
    Camera,
    Lens,
    FileType,
    HasLocation,
    Place,
    Person,
    Folder,
    Edited,
    HasVersions,
    Group,
}

impl RuleKind {
    /// Every kind a rule row can be switched to (a group is added with
    /// its own button rather than picked from the row's menu).
    pub const ROWS: [RuleKind; 14] = [
        RuleKind::Rating,
        RuleKind::Flag,
        RuleKind::Label,
        RuleKind::Keyword,
        RuleKind::Taken,
        RuleKind::Camera,
        RuleKind::Lens,
        RuleKind::FileType,
        RuleKind::HasLocation,
        RuleKind::Place,
        RuleKind::Person,
        RuleKind::Folder,
        RuleKind::Edited,
        RuleKind::HasVersions,
    ];

    /// A fresh rule of this kind with neutral defaults.
    pub fn default_rule(self) -> Rule {
        match self {
            RuleKind::Rating => Rule::Rating {
                compare: Compare::AtLeast,
                stars: 3,
            },
            RuleKind::Flag => Rule::Flag {
                flag: CullFlag::Pick,
            },
            RuleKind::Label => Rule::Label {
                label: ColourLabel::Red,
            },
            RuleKind::Keyword => Rule::Keyword {
                text: String::new(),
            },
            RuleKind::Taken => Rule::Taken {
                from: String::new(),
                to: String::new(),
            },
            RuleKind::Camera => Rule::Camera {
                text: String::new(),
            },
            RuleKind::Lens => Rule::Lens {
                text: String::new(),
            },
            RuleKind::FileType => Rule::FileType {
                file: FileKind::Raw,
            },
            RuleKind::HasLocation => Rule::HasLocation { yes: true },
            RuleKind::Place => Rule::Place {
                text: String::new(),
            },
            RuleKind::Person => Rule::Person {
                name: String::new(),
            },
            RuleKind::Folder => Rule::Folder {
                path: PathBuf::new(),
            },
            RuleKind::Edited => Rule::Edited { yes: true },
            RuleKind::HasVersions => Rule::HasVersions { yes: true },
            RuleKind::Group => Rule::Group(RuleGroup {
                matching: Match::Any,
                rules: Vec::new(),
            }),
        }
    }
}

/// What the rules are asked about one photo. Gathered by the app from
/// its in-memory index; `has_versions` is only looked up when a rule
/// asks for it, since it means listing a directory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhotoFacts {
    pub path: PathBuf,
    pub culling: PhotoCulling,
    /// XMP keywords, as written.
    pub keywords: Vec<String>,
    /// Sortable "YYYY-MM-DD HH:MM:SS": EXIF (or XMP) capture time, the
    /// file's clock when the photo has none.
    pub taken: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub has_location: bool,
    pub place: Option<String>,
    /// The people tagged in the photo, by name.
    pub people: Vec<String>,
    pub edited: bool,
    pub has_versions: bool,
}

fn contains_folded(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

impl Rule {
    pub fn kind(&self) -> RuleKind {
        match self {
            Rule::Rating { .. } => RuleKind::Rating,
            Rule::Flag { .. } => RuleKind::Flag,
            Rule::Label { .. } => RuleKind::Label,
            Rule::Keyword { .. } => RuleKind::Keyword,
            Rule::Taken { .. } => RuleKind::Taken,
            Rule::Camera { .. } => RuleKind::Camera,
            Rule::Lens { .. } => RuleKind::Lens,
            Rule::FileType { .. } => RuleKind::FileType,
            Rule::HasLocation { .. } => RuleKind::HasLocation,
            Rule::Place { .. } => RuleKind::Place,
            Rule::Person { .. } => RuleKind::Person,
            Rule::Folder { .. } => RuleKind::Folder,
            Rule::Edited { .. } => RuleKind::Edited,
            Rule::HasVersions { .. } => RuleKind::HasVersions,
            Rule::Group(_) => RuleKind::Group,
        }
    }

    /// Whether the rule holds for a photo; `None` when the rule says
    /// nothing yet (blank text, an empty group) and is to be ignored.
    pub fn eval(&self, facts: &PhotoFacts) -> Option<bool> {
        let text = |t: &str| -> Option<String> {
            let t = t.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        Some(match self {
            Rule::Rating { compare, stars } => {
                let r = facts.culling.rating;
                match compare {
                    Compare::AtLeast => r >= *stars,
                    Compare::AtMost => r <= *stars,
                    Compare::Exactly => r == *stars,
                }
            }
            Rule::Flag { flag } => facts.culling.flag == *flag,
            Rule::Label { label } => facts.culling.label == *label,
            Rule::Keyword { text: t } => {
                let t = text(t)?;
                facts.keywords.iter().any(|k| contains_folded(k, &t))
            }
            Rule::Taken { from, to } => {
                let (from, to) = (from.trim(), to.trim());
                if from.is_empty() && to.is_empty() {
                    return None;
                }
                let Some(taken) = &facts.taken else {
                    return Some(false);
                };
                // A prefix comparison makes "2024" mean the whole year
                // at either end: from "2024" admits 2024-01-01, to
                // "2024" admits 2024-12-31.
                let after = from.is_empty() || taken.as_str() >= from;
                let before = to.is_empty() || {
                    let head = taken.get(..to.len()).unwrap_or(taken);
                    head <= to
                };
                after && before
            }
            Rule::Camera { text: t } => {
                let t = text(t)?;
                facts
                    .camera
                    .as_deref()
                    .is_some_and(|c| contains_folded(c, &t))
            }
            Rule::Lens { text: t } => {
                let t = text(t)?;
                facts
                    .lens
                    .as_deref()
                    .is_some_and(|c| contains_folded(c, &t))
            }
            Rule::FileType { file } => FileKind::of(&facts.path) == *file,
            Rule::HasLocation { yes } => facts.has_location == *yes,
            Rule::Place { text: t } => {
                let t = text(t)?;
                facts
                    .place
                    .as_deref()
                    .is_some_and(|c| contains_folded(c, &t))
            }
            Rule::Person { name } => {
                let name = text(name)?;
                facts.people.iter().any(|p| p.eq_ignore_ascii_case(&name))
            }
            Rule::Folder { path } => {
                if path.as_os_str().is_empty() {
                    return None;
                }
                facts.path.starts_with(path)
            }
            Rule::Edited { yes } => facts.edited == *yes,
            Rule::HasVersions { yes } => facts.has_versions == *yes,
            Rule::Group(group) => return group.eval(facts),
        })
    }
}

/// A list of rules and how they combine; groups nest.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RuleGroup {
    #[serde(default)]
    pub matching: Match,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl RuleGroup {
    /// The group's verdict, `None` when none of its rules says anything.
    pub fn eval(&self, facts: &PhotoFacts) -> Option<bool> {
        let mut verdicts = self.rules.iter().filter_map(|r| r.eval(facts)).peekable();
        verdicts.peek()?;
        Some(match self.matching {
            Match::All => verdicts.all(|v| v),
            Match::Any => verdicts.any(|v| v),
            Match::None => !verdicts.any(|v| v),
        })
    }

    /// Whether a photo belongs. An album with no usable rule holds
    /// nothing — an empty smart album is one still being written, not
    /// a second "All photos".
    pub fn matches(&self, facts: &PhotoFacts) -> bool {
        self.eval(facts) == Some(true)
    }

    /// Whether any rule, at any depth, is of this kind — so the costly
    /// facts are only gathered for albums that ask about them.
    pub fn uses(&self, kind: RuleKind) -> bool {
        self.rules.iter().any(|r| match r {
            Rule::Group(g) => g.uses(kind),
            r => r.kind() == kind,
        })
    }

    /// How many rules, groups included, at every depth.
    pub fn len(&self) -> usize {
        self.rules
            .iter()
            .map(|r| match r {
                Rule::Group(g) => 1 + g.len(),
                _ => 1,
            })
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// The sidecar names in a `.schist/versions` directory that have at
/// least one kept version: `<stamp>-<sidecar>` gives `<sidecar>`. One
/// listing answers "has versions" for every photo in the folder.
pub fn versioned_sidecars(dir: &Path) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Ok(entries) = std::fs::read_dir(dir.join(".schist").join("versions")) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(sidecar) = crate::versions::sidecar_of_version(name) {
            out.insert(sidecar.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(path: &str) -> PhotoFacts {
        PhotoFacts {
            path: PathBuf::from(path),
            ..Default::default()
        }
    }

    fn group(matching: Match, rules: Vec<Rule>) -> RuleGroup {
        RuleGroup { matching, rules }
    }

    #[test]
    fn each_predicate_reads_its_own_fact() {
        let mut p = photo("/lib/2024/trip/IMG_1.CR3");
        p.culling = PhotoCulling {
            rating: 4,
            flag: CullFlag::Pick,
            label: ColourLabel::Green,
        };
        p.keywords = vec!["Beach".into(), "Family holiday".into()];
        p.taken = Some("2024-07-14 10:00:00".into());
        p.camera = Some("Canon EOS R5".into());
        p.lens = Some("RF24-70mm F2.8 L IS USM".into());
        p.has_location = true;
        p.place = Some("Brighton".into());
        p.people = vec!["Ann".into()];
        p.edited = true;
        p.has_versions = false;
        let holds = |rule: Rule| rule.eval(&p);
        let rating = |compare, stars| Rule::Rating { compare, stars };
        assert_eq!(holds(rating(Compare::AtLeast, 4)), Some(true));
        assert_eq!(holds(rating(Compare::AtLeast, 5)), Some(false));
        assert_eq!(holds(rating(Compare::AtMost, 3)), Some(false));
        assert_eq!(holds(rating(Compare::Exactly, 4)), Some(true));
        assert_eq!(
            holds(Rule::Flag {
                flag: CullFlag::Pick
            }),
            Some(true)
        );
        assert_eq!(
            holds(Rule::Flag {
                flag: CullFlag::Reject
            }),
            Some(false)
        );
        assert_eq!(
            holds(Rule::Label {
                label: ColourLabel::Green
            }),
            Some(true)
        );
        assert_eq!(
            holds(Rule::Label {
                label: ColourLabel::Red
            }),
            Some(false)
        );
        assert_eq!(
            holds(Rule::Keyword {
                text: "beach".into()
            }),
            Some(true)
        );
        assert_eq!(
            holds(Rule::Keyword {
                text: "holiday".into()
            }),
            Some(true)
        );
        assert_eq!(
            holds(Rule::Keyword {
                text: "snow".into()
            }),
            Some(false)
        );
        let taken = |from: &str, to: &str| Rule::Taken {
            from: from.into(),
            to: to.into(),
        };
        assert_eq!(holds(taken("2024-07-01", "2024-07-31")), Some(true));
        assert_eq!(holds(taken("2024", "2024")), Some(true));
        assert_eq!(holds(taken("2024-07-14", "2024-07-14")), Some(true));
        assert_eq!(holds(taken("", "2024-06")), Some(false));
        assert_eq!(holds(taken("2024-08", "")), Some(false));
        assert_eq!(
            holds(Rule::Camera {
                text: "eos r5".into()
            }),
            Some(true)
        );
        assert_eq!(
            holds(Rule::Camera {
                text: "nikon".into()
            }),
            Some(false)
        );
        assert_eq!(
            holds(Rule::Lens {
                text: "24-70".into()
            }),
            Some(true)
        );
        assert_eq!(
            holds(Rule::FileType {
                file: FileKind::Raw
            }),
            Some(true)
        );
        assert_eq!(
            holds(Rule::FileType {
                file: FileKind::Jpeg
            }),
            Some(false)
        );
        assert_eq!(holds(Rule::HasLocation { yes: true }), Some(true));
        assert_eq!(holds(Rule::HasLocation { yes: false }), Some(false));
        assert_eq!(
            holds(Rule::Place {
                text: "bright".into()
            }),
            Some(true)
        );
        assert_eq!(holds(Rule::Person { name: "ann".into() }), Some(true));
        assert_eq!(
            holds(Rule::Person {
                name: "Anna".into()
            }),
            Some(false)
        );
        assert_eq!(
            holds(Rule::Folder {
                path: "/lib/2024".into()
            }),
            Some(true)
        );
        assert_eq!(
            holds(Rule::Folder {
                path: "/lib/2023".into()
            }),
            Some(false)
        );
        assert_eq!(holds(Rule::Edited { yes: true }), Some(true));
        assert_eq!(holds(Rule::HasVersions { yes: true }), Some(false));
        assert_eq!(holds(Rule::HasVersions { yes: false }), Some(true));
    }

    #[test]
    fn missing_facts_do_not_match_and_blank_rules_say_nothing() {
        let p = photo("/a.png");
        assert_eq!(
            Rule::Camera {
                text: "canon".into()
            }
            .eval(&p),
            Some(false)
        );
        assert_eq!(
            Rule::Taken {
                from: "2020".into(),
                to: String::new()
            }
            .eval(&p),
            Some(false)
        );
        for blank in [
            Rule::Keyword { text: " ".into() },
            Rule::Camera {
                text: String::new(),
            },
            Rule::Taken {
                from: String::new(),
                to: String::new(),
            },
            Rule::Person {
                name: String::new(),
            },
            Rule::Folder {
                path: PathBuf::new(),
            },
            Rule::Group(RuleGroup::default()),
        ] {
            assert_eq!(blank.eval(&p), None, "{blank:?}");
        }
    }

    #[test]
    fn all_any_none_and_nesting() {
        let mut p = photo("/a.jpg");
        p.culling.rating = 5;
        p.culling.flag = CullFlag::Reject;
        let five = Rule::Rating {
            compare: Compare::AtLeast,
            stars: 5,
        };
        let picked = Rule::Flag {
            flag: CullFlag::Pick,
        };
        assert!(!group(Match::All, vec![five.clone(), picked.clone()]).matches(&p));
        assert!(group(Match::Any, vec![five.clone(), picked.clone()]).matches(&p));
        assert!(!group(Match::None, vec![five.clone(), picked.clone()]).matches(&p));
        assert!(group(Match::None, vec![picked.clone()]).matches(&p));
        // Five stars and (picked or a JPEG).
        let nested = group(
            Match::All,
            vec![
                five.clone(),
                Rule::Group(group(
                    Match::Any,
                    vec![
                        picked.clone(),
                        Rule::FileType {
                            file: FileKind::Jpeg,
                        },
                    ],
                )),
            ],
        );
        assert!(nested.matches(&p));
        assert!(nested.uses(RuleKind::FileType));
        assert!(!nested.uses(RuleKind::HasVersions));
        assert_eq!(nested.len(), 4);
        // A blank rule beside a real one is ignored, not a veto.
        let half_written = group(
            Match::All,
            vec![
                five,
                Rule::Keyword {
                    text: String::new(),
                },
            ],
        );
        assert!(half_written.matches(&p));
    }

    #[test]
    fn an_empty_album_holds_nothing() {
        let p = photo("/a.jpg");
        assert!(!RuleGroup::default().matches(&p));
        let blank = group(
            Match::Any,
            vec![Rule::Keyword {
                text: String::new(),
            }],
        );
        assert!(!blank.matches(&p));
        assert!(!group(Match::None, Vec::new()).matches(&p));
    }

    #[test]
    fn rules_round_trip_through_json_and_old_shapes_default() {
        let rules = group(
            Match::Any,
            vec![
                Rule::Rating {
                    compare: Compare::Exactly,
                    stars: 2,
                },
                Rule::Label {
                    label: ColourLabel::Blue,
                },
                Rule::Group(group(Match::None, vec![Rule::Edited { yes: false }])),
                Rule::FileType {
                    file: FileKind::JpegXl,
                },
            ],
        );
        let text = serde_json::to_string(&rules).unwrap();
        assert_eq!(serde_json::from_str::<RuleGroup>(&text).unwrap(), rules);
        // Defaults fill what a hand-written file leaves out.
        let terse: RuleGroup = serde_json::from_str(
            r#"{"rules":[{"kind":"has_location"},{"kind":"rating","stars":4}]}"#,
        )
        .unwrap();
        assert_eq!(terse.matching, Match::All);
        assert_eq!(terse.rules[0], Rule::HasLocation { yes: true });
        assert_eq!(
            terse.rules[1],
            Rule::Rating {
                compare: Compare::AtLeast,
                stars: 4
            }
        );
    }

    #[test]
    fn file_kinds_come_from_the_extension() {
        let kind = |p: &str| FileKind::of(Path::new(p));
        assert_eq!(kind("a.JPG"), FileKind::Jpeg);
        assert_eq!(kind("a.heic"), FileKind::Heif);
        assert_eq!(kind("a.dng"), FileKind::Raw);
        assert_eq!(kind("a.NEF"), FileKind::Raw);
        assert_eq!(kind("a.psd"), FileKind::Layered);
        assert_eq!(kind("a.mov"), FileKind::Video);
        assert_eq!(kind("a.jxl"), FileKind::JpegXl);
        assert_eq!(kind("a.bmp"), FileKind::Other);
        assert_eq!(kind("noext"), FileKind::Other);
    }

    #[test]
    fn kept_versions_are_found_per_folder() {
        let dir =
            std::env::temp_dir().join(format!("schist-smart-versions-{}", std::process::id()));
        let versions = dir.join(".schist").join("versions");
        std::fs::create_dir_all(&versions).unwrap();
        std::fs::write(versions.join("1700000000-a.jpg.psd"), b"").unwrap();
        std::fs::write(versions.join("1700000001.2-a.jpg.psd"), b"").unwrap();
        std::fs::write(versions.join("1700000002-b-c.png.psd"), b"").unwrap();
        std::fs::write(versions.join("notes.txt"), b"").unwrap();
        let found = versioned_sidecars(&dir);
        assert!(found.contains("a.jpg.psd"));
        assert!(found.contains("b-c.png.psd"));
        assert_eq!(found.len(), 2);
        assert!(versioned_sidecars(&dir.join("missing")).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
