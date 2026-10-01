//! Language resources retain native identities independently of shaping tags.
use serde::{Deserialize, Serialize};

/// Native identities and directly authored tags occupy separate namespaces.
/// The string form preserves old documents and native IDML references. Its
/// legacy unqualified tags resolve only after declared identities. New edits
/// use Tag, so an opaque native ID named "tr" cannot intercept Turkish text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TextLanguage {
    Tag { tag: String },
    Reference(String),
}

impl TextLanguage {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Tag { tag } => tag,
            Self::Reference(value) => value,
        }
    }
}

impl From<String> for TextLanguage {
    fn from(value: String) -> Self {
        Self::Reference(value)
    }
}

impl From<&str> for TextLanguage {
    fn from(value: &str) -> Self {
        Self::Reference(value.into())
    }
}

impl std::fmt::Display for TextLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LanguageResource {
    pub id: String,
    pub name: String,
    pub primary_name: Option<String>,
    pub sublanguage_name: Option<String>,
    pub language_id: Option<i32>,
    pub single_quotes: Option<String>,
    pub double_quotes: Option<String>,
    pub hyphenation_vendor: Option<String>,
    pub spelling_vendor: Option<String>,
    pub labels: Vec<(String, String)>,
}

impl LanguageResource {
    pub fn tag(&self) -> Option<String> {
        native_tag(&self.name).or_else(|| self.primary_name.as_deref().and_then(native_tag))
    }
}

impl crate::StyleSet {
    pub fn resolve_language(&self, value: &TextLanguage) -> Option<String> {
        match value {
            TextLanguage::Tag { tag } => schist_text_engine::normalize_language(tag),
            TextLanguage::Reference(value) => self.language_tag(value),
        }
    }
    /// Resolve declared identities before interpreting a directly authored tag.
    /// A native ID is opaque: its spelling is never used to guess its language.
    pub fn language_tag(&self, value: &str) -> Option<String> {
        if let Some(resource) = self
            .languages
            .iter()
            .find(|r| r.id == value)
            .or_else(|| self.languages.iter().find(|r| r.name == value))
        {
            return resource.tag();
        }
        native_tag(value)
    }
}

/// Built-in names observed in public IDML resources. Preserve the full native
/// resource for dictionaries, quotation marks and orthography preferences.
pub fn native_tag(name: &str) -> Option<String> {
    let native = name.starts_with("$ID/");
    let name = name.strip_prefix("$ID/").unwrap_or(name);
    let tag = match name {
        "[No Language]" => "und",
        "Arabic" => "ar",
        "Bulgarian" => "bg",
        "Catalan" => "ca",
        "Croatian" => "hr",
        "Czech" => "cs",
        "Danish" => "da",
        "Dutch" => "nl",
        "English: USA" | "English: USA Medical" | "English: USA Legal" => "en-US",
        "English: UK" => "en-GB",
        "English: Canadian" => "en-CA",
        "English" => "en",
        "Estonian" => "et",
        "Finnish" => "fi",
        "French" => "fr",
        "French: Canadian" => "fr-CA",
        "German" | "German: Traditional" | "German: Reformed" => "de",
        "German: Swiss" => "de-CH",
        "German: Austrian" => "de-AT",
        "Greek" => "el",
        "Hebrew" => "he",
        "Hungarian" => "hu",
        "Italian" => "it",
        "Japanese" => "ja",
        "Latvian" => "lv",
        "Lithuanian" => "lt",
        "Norwegian: Bokmal" => "nb",
        "Norwegian: Nynorsk" => "nn",
        "Polish" => "pl",
        "Portuguese" | "Portuguese: Orthographic Agreement" => "pt",
        "Portuguese: Brazilian" => "pt-BR",
        "Romanian" => "ro",
        "Russian" => "ru",
        "Slovak" => "sk",
        "Slovenian" => "sl",
        "Spanish: Castilian" => "es-ES",
        "Swedish" => "sv",
        "Thai" => "th",
        "Turkish" => "tr",
        "Ukrainian" => "uk",
        other if native && other.split(['-', '_']).next().is_some_and(|s| s.len() > 3) => {
            return None
        }
        other => other,
    };
    schist_text_engine::normalize_language(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_language_identity_wins_over_tag_shaped_ids_and_does_not_drop_vendor_settings() {
        let resource = LanguageResource {
            id: "tr".into(),
            name: "$ID/Romanian".into(),
            hyphenation_vendor: Some("Example dictionary".into()),
            single_quotes: Some("‚‘".into()),
            ..Default::default()
        };
        let styles = crate::StyleSet {
            languages: vec![
                LanguageResource {
                    id: "other".into(),
                    name: "tr".into(),
                    ..Default::default()
                },
                resource.clone(),
            ],
            ..Default::default()
        };
        assert_eq!(styles.language_tag("tr").as_deref(), Some("ro"));
        assert_eq!(styles.language_tag("$ID/Romanian").as_deref(), Some("ro"));
        assert_eq!(
            styles.language_tag("az-Latn-AZ").as_deref(),
            Some("az-latn-az")
        );
        assert_eq!(styles.language_tag("Language/$ID/Turkish"), None);
        let mut unknown = styles.clone();
        unknown.languages[1].name = "$ID/Unrecognized native language".into();
        assert_eq!(unknown.language_tag("tr"), None);
        assert_eq!(
            serde_json::from_str::<LanguageResource>(&serde_json::to_string(&resource).unwrap())
                .unwrap(),
            resource
        );
    }
}
