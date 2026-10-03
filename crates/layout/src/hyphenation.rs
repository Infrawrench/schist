//! Native automatic-hyphenation options, independent of the paragraph's enable
//! switch. Unset values inherit; disabling hyphenation retains the policy.
//!
//! These options are retained for interchange. Dictionary break selection and
//! composition constraints are not implemented by this representation.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HyphenationOptions {
    pub capitalized_words: Option<bool>,
    pub last_word: Option<bool>,
    pub across_columns: Option<bool>,
    /// Minimum letters before and after a dictionary break, respectively.
    pub after_first: Option<u8>,
    pub before_last: Option<u8>,
    /// Minimum word length eligible for automatic hyphenation (inclusive).
    pub words_longer_than: Option<u8>,
    /// Maximum consecutive hyphenated lines; zero explicitly means unlimited.
    pub ladder_limit: Option<u8>,
    /// Allowed end-of-line whitespace in points for a single-line composer.
    pub zone: Option<f32>,
    /// Native spacing-versus-hyphens preference, retained without rescaling.
    /// The published prose/current DOM say 0–100; the older RNC says 0–10.
    pub weight: Option<u8>,
}

impl HyphenationOptions {
    pub fn over(&self, base: &Self) -> Self {
        Self {
            capitalized_words: self.capitalized_words.or(base.capitalized_words),
            last_word: self.last_word.or(base.last_word),
            across_columns: self.across_columns.or(base.across_columns),
            after_first: self.after_first.or(base.after_first),
            before_last: self.before_last.or(base.before_last),
            words_longer_than: self.words_longer_than.or(base.words_longer_than),
            ladder_limit: self.ladder_limit.or(base.ladder_limit),
            zone: self.zone.or(base.zone),
            weight: self.weight.or(base.weight),
        }
    }
}
