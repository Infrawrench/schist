//! Public designmap Language resources and AppliedLanguage references.
use crate::{designmap::DesignPackage, error::Error, import::Report, xml};
use schist_layout::language::{LanguageResource, TextLanguage};
use serde::{Deserialize, Serialize};

const LABEL: &str = "Schist.Language.v1";

fn resource<'a>(resources: &'a [LanguageResource], value: &str) -> Option<&'a LanguageResource> {
    resources
        .iter()
        .find(|r| r.id == value)
        .or_else(|| resources.iter().find(|r| r.name == value))
}

#[derive(Serialize, Deserialize)]
struct Authored {
    value: TextLanguage,
    native: LanguageResource,
}

pub(crate) fn applied(
    element: &xml::Element,
    refs: &[LanguageResource],
    report: &mut Report,
) -> Option<TextLanguage> {
    let value = element.attr("AppliedLanguage")?;
    if let Some(raw) = crate::auto_direction::label(element, LABEL) {
        match serde_json::from_str::<Authored>(raw) {
            Ok(authored)
                if value == authored.native.id
                    && resource(refs, value) == Some(&authored.native) =>
            {
                return Some(authored.value);
            }
            Ok(_) => {} // Native reference or dictionary edits supersede the saved intent.
            Err(_) => report.skip(schist_i18n::tf!(
                "design.idml_language_invalid",
                value = raw
            )),
        }
    }
    let tag = match resource(refs, value) {
        Some(r) => r.tag(),
        None => schist_layout::language::native_tag(value),
    };
    if tag.is_none() {
        report.skip(schist_i18n::tf!(
            "design.idml_language_unresolved",
            value = value
        ));
    }
    Some(value.into())
}

/// Native names observed in the public fixture inventory. Do not discard
/// arbitrary script/region subtags just to force a tag into a native dictionary.
fn native_name(value: &str) -> Option<&'static str> {
    Some(match value {
        "" | "und" => "[No Language]",
        "ar" => "Arabic",
        "bg" => "Bulgarian",
        "ca" => "Catalan",
        "hr" => "Croatian",
        "cs" => "Czech",
        "da" => "Danish",
        "nl" => "Dutch",
        "en-us" => "English: USA",
        "en-gb" => "English: UK",
        "en-ca" => "English: Canadian",
        "et" => "Estonian",
        "fi" => "Finnish",
        "fr" => "French",
        "fr-ca" => "French: Canadian",
        "de" => "German: Reformed",
        "de-ch" => "German: Swiss",
        "de-at" => "German: Austrian",
        "el" => "Greek",
        "he" => "Hebrew",
        "hu" => "Hungarian",
        "it" => "Italian",
        "ja" => "Japanese",
        "lv" => "Latvian",
        "lt" | "lt-lt" => "Lithuanian",
        "nb" => "Norwegian: Bokmal",
        "nn" => "Norwegian: Nynorsk",
        "pl" => "Polish",
        "pt" => "Portuguese",
        "pt-br" => "Portuguese: Brazilian",
        "ro" | "ro-ro" => "Romanian",
        "ru" => "Russian",
        "sk" => "Slovak",
        "sl" => "Slovenian",
        "es-es" => "Spanish: Castilian",
        "sv" => "Swedish",
        "th" => "Thai",
        "tr" | "tr-tr" => "Turkish",
        "uk" => "Ukrainian",
        "bn-in" => "bn_IN",
        "gu-in" => "gu_IN",
        "hi-in" => "hi_IN",
        "kn-in" => "kn_IN",
        "ml-in" => "ml_IN",
        "mr-in" => "mr_IN",
        "or-in" => "or_IN",
        "pa-in" => "pa_IN",
        "ta-in" => "ta_IN",
        "te-in" => "te_IN",
        "id-id" => "id_ID",
        "km-kh" => "km_KH",
        "lo-la" => "lo_LA",
        "my-mm" => "my_MM",
        "si-lk" => "si_LK",
        _ => return None,
    })
}

pub(crate) struct ExportLanguages {
    pub resources: Vec<LanguageResource>,
    lowered: std::collections::BTreeMap<TextLanguage, (String, Option<String>)>,
}

impl ExportLanguages {
    pub fn new(styles: &schist_layout::StyleSet, warnings: &mut Vec<String>) -> Self {
        let mut out = Self {
            resources: styles.languages.clone(),
            lowered: Default::default(),
        };
        for value in styles
            .paragraphs
            .iter()
            .filter_map(|s| s.language.as_ref())
            .chain(styles.characters.iter().filter_map(|s| s.language.as_ref()))
        {
            if out.lowered.contains_key(value) {
                continue;
            }
            // Explicit tags bypass native identities, even when spellings collide.
            if let TextLanguage::Reference(value_string) = value {
                if let Some(r) = resource(&styles.languages, value_string) {
                    if r.tag().is_none() {
                        warnings.push(schist_i18n::tf!(
                            "design.idml_language_unresolved",
                            value = value
                        ));
                    }
                    out.lowered
                        .insert(value.clone(), (value_string.clone(), None));
                    continue;
                }
            }
            let tag = match value {
                TextLanguage::Tag { tag } => schist_text_engine::normalize_language(tag),
                TextLanguage::Reference(value) => schist_layout::language::native_tag(value),
            };
            let known = tag.as_deref().and_then(native_name);
            let raw = value.as_str();
            let name = if matches!(value, TextLanguage::Reference(_))
                && raw.starts_with("$ID/")
                && known.is_some()
            {
                raw.to_owned()
            } else {
                format!("$ID/{}", known.unwrap_or("[No Language]"))
            };
            if known.is_none() && !raw.is_empty() {
                warnings.push(schist_i18n::tf!(
                    "design.idml_language_private",
                    value = value
                ));
            }
            let native = if let Some(r) = out.resources.iter().find(|r| r.name == name) {
                r.clone()
            } else {
                // Self values are opaque and must not collide with imported IDs.
                let mut index = out.resources.len();
                let id = loop {
                    let candidate = format!("SchistLanguage{index}");
                    if !out
                        .resources
                        .iter()
                        .any(|r| r.id == candidate || r.name == candidate)
                        && !styles.numbering_lists.iter().any(|r| r.id == candidate)
                        && !styles
                            .paragraphs
                            .iter()
                            .filter_map(|s| s.language.as_ref())
                            .chain(styles.characters.iter().filter_map(|s| s.language.as_ref()))
                            .any(|value| value.as_str() == candidate)
                        && !styles
                            .paragraphs
                            .iter()
                            .any(|s| s.list.list.as_deref() == Some(candidate.as_str()))
                    {
                        break candidate;
                    }
                    index += 1;
                };
                let r = LanguageResource {
                    id,
                    name,
                    ..Default::default()
                };
                out.resources.push(r.clone());
                r
            };
            let label = serde_json::to_string(&Authored {
                value: value.clone(),
                native: native.clone(),
            })
            .expect("language metadata");
            out.lowered.insert(value.clone(), (native.id, Some(label)));
        }
        out
    }

    pub fn native(&self, value: &Option<TextLanguage>) -> Option<TextLanguage> {
        value.as_ref().map(|v| self.lowered[v].0.clone().into())
    }

    pub fn label(&self, xml: &mut String, value: &Option<TextLanguage>) {
        let Some(label) = value.as_ref().and_then(|v| self.lowered[v].1.as_ref()) else {
            return;
        };
        let pair = format!(
            "<KeyValuePair Key=\"{LABEL}\" Value=\"{}\"/>",
            crate::export::escape(label)
        );
        if let Some(at) = xml.find("</Label>") {
            xml.insert_str(at, &pair);
        } else if let Some(at) = xml.find("</Properties>") {
            xml.insert_str(at, &format!("<Label>{pair}</Label>"));
        }
    }
}

pub(crate) fn read(
    opened: &DesignPackage<'_>,
    report: &mut Report,
) -> Result<Vec<LanguageResource>, Error> {
    let root = xml::parse(opened.text_of(&opened.root)?).map_err(|message| Error::Xml {
        part: opened.root.clone(),
        message,
    })?;
    let mut resources = Vec::new();
    for element in root.children_named("Language") {
        let (Some(id), Some(name)) = (element.attr("Self"), element.attr("Name")) else {
            report.skip(schist_i18n::tf!(
                "design.idml_language_invalid",
                value = element.attr("Self").unwrap_or_default()
            ));
            continue;
        };
        if id.is_empty()
            || name.is_empty()
            || resources.iter().any(|r: &LanguageResource| r.id == id)
        {
            report.skip(schist_i18n::tf!("design.idml_language_invalid", value = id));
            continue;
        }
        let language_id = element.attr("Id").and_then(|v| v.parse().ok());
        if element.attr("Id").is_some() && language_id.is_none() {
            report.skip(schist_i18n::tf!("design.idml_language_invalid", value = id));
        }
        resources.push(LanguageResource {
            id: id.into(),
            name: name.into(),
            language_id,
            primary_name: element.attr("PrimaryLanguageName").map(str::to_owned),
            sublanguage_name: element.attr("SublanguageName").map(str::to_owned),
            single_quotes: element.attr("SingleQuotes").map(str::to_owned),
            double_quotes: element.attr("DoubleQuotes").map(str::to_owned),
            hyphenation_vendor: element.attr("HyphenationVendor").map(str::to_owned),
            spelling_vendor: element.attr("SpellingVendor").map(str::to_owned),
            labels: element
                .child("Properties")
                .and_then(|p| p.child("Label"))
                .into_iter()
                .flat_map(|label| label.children_named("KeyValuePair"))
                .filter_map(|pair| {
                    Some((pair.attr("Key")?.to_owned(), pair.attr("Value")?.to_owned()))
                })
                .collect(),
        });
    }
    Ok(resources)
}

pub(crate) fn resources(resources: &[LanguageResource]) -> String {
    let escape = crate::export::escape;
    let mut out = String::new();
    for language in resources {
        out.push_str(&format!(
            r#"<Language Self="{}" Name="{}""#,
            escape(&language.id),
            escape(&language.name)
        ));
        for (key, value) in [
            ("PrimaryLanguageName", &language.primary_name),
            ("SublanguageName", &language.sublanguage_name),
            ("SingleQuotes", &language.single_quotes),
            ("DoubleQuotes", &language.double_quotes),
            ("HyphenationVendor", &language.hyphenation_vendor),
            ("SpellingVendor", &language.spelling_vendor),
        ] {
            if let Some(value) = value {
                out.push_str(&format!(r#" {key}="{}""#, escape(value)));
            }
        }
        if let Some(id) = language.language_id {
            out.push_str(&format!(r#" Id="{id}""#));
        }
        if language.labels.is_empty() {
            out.push_str("/>");
        } else {
            out.push_str("><Properties><Label>");
            for (key, value) in &language.labels {
                out.push_str(&format!(
                    r#"<KeyValuePair Key="{}" Value="{}"/>"#,
                    escape(key),
                    escape(value)
                ));
            }
            out.push_str("</Label></Properties></Language>");
        }
    }
    out
}
