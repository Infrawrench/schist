use schist_layout::{
    hyphenation::{opportunities, Dictionary, HyphenationOptions},
    language::{LanguageResource, TextLanguage},
    CharacterStyle, ResolvedCharacter, ResolvedParagraph, Story, StoryPoint, StyleRange, StyleSet,
};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

fn tag(value: &str) -> TextLanguage {
    TextLanguage::Tag { tag: value.into() }
}

fn source(text: &str) -> Story {
    Story {
        points: vec![StoryPoint::Paragraph {
            text: text.into(),
            style: "P".into(),
        }],
        ..Story::new()
    }
}

fn character() -> ResolvedCharacter {
    ResolvedCharacter {
        language: Some(tag("en-US")),
        ..Default::default()
    }
}

fn breaks(story: &Story, styles: &StyleSet, paragraph: &ResolvedParagraph) -> Vec<usize> {
    opportunities(story, 0..story.text_len(), styles, paragraph, &character())
}

#[test]
fn dictionary_resolution_preserves_region_script_orthography_and_opaque_id_namespaces() {
    let mut styles = StyleSet::default();
    for (dictionary, names) in [
        (
            Dictionary::EnglishUs,
            vec!["en-US", "EN_latn_US", "$ID/English: USA"],
        ),
        (
            Dictionary::French,
            vec!["fr", "fr-FR", "fr-Latn-FR", "$ID/French"],
        ),
        (
            Dictionary::GermanReformed,
            vec![
                "de-1996",
                "de-DE-1996",
                "de-Latn-DE-1996",
                "$ID/German: Reformed",
            ],
        ),
    ] {
        for name in names {
            assert_eq!(
                Dictionary::resolve(&styles, &name.into()),
                Some(dictionary),
                "{name}"
            );
            styles.languages.push(LanguageResource {
                id: "opaque".into(),
                name: name.into(),
                ..Default::default()
            });
            assert_eq!(
                Dictionary::resolve(&styles, &"opaque".into()),
                Some(dictionary)
            );
            styles.languages.clear();
        }
    }
    for name in [
        "",
        "und",
        "en",
        "en-GB",
        "en-CA",
        "en-Cyrl-US",
        "en-US-x-private",
        "fr-CA",
        "de",
        "de-CH",
        "de-1901",
        "$ID/German: Traditional",
        "$ID/English: USA Medical",
        "$ID/Unknown",
        "Language/$ID/French",
    ] {
        assert_eq!(Dictionary::resolve(&styles, &name.into()), None, "{name}");
        styles.languages.push(LanguageResource {
            id: "en-US".into(),
            name: name.into(),
            primary_name: Some("$ID/English: USA".into()),
            ..Default::default()
        });
        assert_eq!(
            Dictionary::resolve(&styles, &"en-US".into()),
            None,
            "opaque {name}"
        );
        assert_eq!(
            Dictionary::resolve(&styles, &tag("en-US")),
            Some(Dictionary::EnglishUs)
        );
        styles.languages.clear();
    }
}

#[test]
fn word_limits_are_inclusive_and_only_remove_original_dictionary_opportunities() {
    for (dictionary, word) in [
        (Dictionary::EnglishUs, "extensive"),
        (Dictionary::French, "extraordinaire"),
        (Dictionary::GermanReformed, "Eingabeaufforderung"),
    ] {
        let count = word.graphemes(true).count();
        let base = dictionary.word_breaks(
            word,
            &HyphenationOptions {
                after_first: Some(1),
                before_last: Some(1),
                ..Default::default()
            },
        );
        assert!(!base.is_empty(), "{word}");
        for minimum in [count as u8, count as u8 + 1] {
            for left in 1..=6 {
                for right in 1..=6 {
                    let actual = dictionary.word_breaks(
                        word,
                        &HyphenationOptions {
                            after_first: Some(left),
                            before_last: Some(right),
                            words_longer_than: Some(minimum),
                            ..Default::default()
                        },
                    );
                    let expected = base
                        .iter()
                        .copied()
                        .filter(|at| {
                            count >= minimum as usize
                                && word[..*at].graphemes(true).count() >= left as usize
                                && word[*at..].graphemes(true).count() >= right as usize
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(actual, expected, "{word} {minimum} {left} {right}");
                }
            }
        }
        let upper = word.to_uppercase();
        assert!(dictionary
            .word_breaks(
                &upper,
                &HyphenationOptions {
                    capitalized_words: Some(false),
                    ..Default::default()
                }
            )
            .is_empty());
        assert!(!dictionary
            .word_breaks(
                &upper,
                &HyphenationOptions {
                    capitalized_words: Some(true),
                    ..Default::default()
                }
            )
            .is_empty());
    }
    assert_eq!(
        Dictionary::EnglishUs.word_breaks("extensive", &Default::default()),
        vec![2, 5]
    );
}

#[test]
fn normalization_and_length_changing_case_map_only_to_original_grapheme_edges() {
    for (dictionary, word) in [
        (Dictionary::French, "réorganisation"),
        (Dictionary::GermanReformed, "Straßenüberführung"),
    ] {
        let policies = [
            HyphenationOptions::default(),
            HyphenationOptions {
                after_first: Some(4),
                before_last: Some(3),
                ..Default::default()
            },
        ];
        for policy in policies {
            let ordinal = |text: &str| {
                dictionary
                    .word_breaks(text, &policy)
                    .iter()
                    .map(|at| text[..*at].graphemes(true).count())
                    .collect::<Vec<_>>()
            };
            let expected = ordinal(word);
            assert!(!expected.is_empty());
            for text in [
                word.nfd().collect::<String>(),
                word.to_lowercase(),
                word.replace('ß', "ẞ"),
            ] {
                assert_eq!(ordinal(&text), expected, "{text}");
                let edges = text
                    .grapheme_indices(true)
                    .map(|(at, _)| at)
                    .collect::<Vec<_>>();
                assert!(dictionary
                    .word_breaks(&text, &policy)
                    .iter()
                    .all(|at| edges.contains(at)));
            }
        }
    }
}

#[test]
fn explicit_hyphens_joiners_numbers_and_foreign_scripts_do_not_gain_dictionary_breaks() {
    let styles = StyleSet::default();
    for text in [
        "ex\u{ad}tensive",
        "ex\u{2060}tensive",
        "ex\u{200d}tensive",
        "extensive1",
        "exтensive",
        "exten-sive",
        "exten'sive",
        "exten’sive",
    ] {
        assert!(
            Dictionary::EnglishUs
                .word_breaks(text, &Default::default())
                .is_empty(),
            "{text}"
        );
    }
    for text in [
        "ex\u{ad}tensive",
        "ex\u{2060}tensive",
        "ex\u{200d}tensive",
        "extensive1",
        "exтensive",
    ] {
        assert!(
            breaks(&source(text), &styles, &Default::default()).is_empty(),
            "{text}"
        );
    }
}

#[test]
fn slicing_never_rehyphenates_a_suffix_or_changes_the_source_story() {
    for (language, text) in [
        ("en-US", "extensive probability"),
        ("fr", "réorganisation extraordinaire"),
        ("de-1996", "Straßenüberführung Eingabeaufforderung"),
    ] {
        let story = source(text);
        let saved = serde_json::to_value(&story).unwrap();
        let styles = StyleSet::default();
        let character = ResolvedCharacter {
            language: Some(tag(language)),
            ..Default::default()
        };
        let expected = opportunities(
            &story,
            0..text.len(),
            &styles,
            &Default::default(),
            &character,
        );
        assert!(!expected.is_empty());
        let boundaries = text
            .char_indices()
            .map(|(at, _)| at)
            .chain([text.len()])
            .collect::<Vec<_>>();
        for &start in &boundaries {
            for &end in boundaries.iter().filter(|end| **end >= start) {
                let actual =
                    opportunities(&story, start..end, &styles, &Default::default(), &character);
                assert_eq!(
                    actual,
                    expected
                        .iter()
                        .copied()
                        .filter(|at| *at > start && *at < end)
                        .map(|at| at - start)
                        .collect::<Vec<_>>(),
                    "{text} {start}..{end}"
                );
            }
        }
        assert_eq!(serde_json::to_value(&story).unwrap(), saved);
    }
}

#[test]
fn mixed_language_and_no_break_ranges_follow_first_matching_run_and_explicit_resets() {
    let mut styles = StyleSet::default();
    for (name, language, no_break) in [
        ("GB", Some(tag("en-GB")), None),
        ("US", Some(tag("en-US")), Some(false)),
        ("Protected", None, Some(true)),
        ("Inherit", None, None),
    ] {
        styles.add_character(CharacterStyle {
            name: name.into(),
            language,
            no_break,
            ..Default::default()
        });
    }
    let mut story = source("extensive extensive");
    let paragraph = ResolvedParagraph::default();
    let expected = breaks(&story, &styles, &paragraph);
    assert_eq!(expected, vec![2, 5, 12, 15]);
    story.ranges.push(StyleRange::new(3, 4, "GB"));
    assert_eq!(breaks(&story, &styles, &paragraph), vec![12, 15]);
    story.ranges.insert(0, StyleRange::new(0, 9, "Inherit"));
    assert_eq!(breaks(&story, &styles, &paragraph), expected);
    story.ranges = vec![StyleRange::new(0, 9, "Protected")];
    assert_eq!(breaks(&story, &styles, &paragraph), vec![12, 15]);
    story.ranges.insert(0, StyleRange::new(0, 9, "US"));
    assert_eq!(breaks(&story, &styles, &paragraph), expected);
}

#[test]
fn disabled_and_final_word_policies_apply_to_full_paragraphs_including_slices() {
    let styles = StyleSet::default();
    let text = "extensive probability!\n";
    let story = source(text);
    let mut paragraph = ResolvedParagraph::default();
    paragraph.hyphenation.last_word = Some(false);
    let expected = breaks(&story, &styles, &paragraph);
    assert_eq!(expected, vec![2, 5]);
    assert!(opportunities(&story, 10..text.len(), &styles, &paragraph, &character()).is_empty());
    paragraph.hyphenate = Some(false);
    assert!(breaks(&story, &styles, &paragraph).is_empty());
}

#[test]
fn long_words_and_empty_input_keep_bounded_valid_offsets_without_panicking() {
    for repeats in [0, 1, 5, 100] {
        let word = "extraordinaire".repeat(repeats);
        let actual = Dictionary::French.word_breaks(&word, &Default::default());
        assert!(actual.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(actual
            .iter()
            .all(|at| *at >= 2 && *at <= word.len() - 2 && word.is_char_boundary(*at)));
    }
}

#[test]
fn a_leading_discretionary_hyphen_protects_only_its_word_even_in_frame_slices() {
    let styles = StyleSet::default();
    for prefix in ["", "a ", "extensive ", "(\""] {
        for leading in ["\u{ad}", "\u{ad}\u{ad}"] {
            let text = format!("{prefix}{leading}extensive! probability");
            let story = source(&text);
            let word_start = prefix.len() + leading.len();
            let word_end = word_start + "extensive".len();
            let all = breaks(&story, &styles, &Default::default());
            assert!(
                all.iter().all(|at| *at < word_start || *at > word_end),
                "{text:?}: {all:?}"
            );
            assert!(all.iter().any(|at| *at > word_end));
            for start in word_start..word_end {
                assert!(
                    opportunities(
                        &story,
                        start..word_end,
                        &styles,
                        &Default::default(),
                        &character()
                    )
                    .is_empty(),
                    "{text:?}: {start}"
                );
            }
        }
    }
}
