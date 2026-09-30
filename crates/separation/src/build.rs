//! Where an ink's CMYK build comes from.
//!
//! An ink's process build decides how much of each of the four process
//! plates it lays down, so it is the input to the whole separation. It is
//! also the one thing the layout model cannot compute well on its own:
//! [`Ink::to_cmyk`] is a preview approximation that zeroes a channel by
//! construction, and under-colour removal withdraws from the smallest
//! channel, so UCR over such a build is silently a no-op.
//!
//! Printing a real plate needs a colour-managed build. The editor owns
//! the ICC machinery, so the build is injected here rather than guessed.

use schist_layout::ink::Ink;

/// An ink's process build, 0..=1 per channel.
pub type Build = [f32; 4];

/// Where CMYK builds come from.
pub trait CmykSource {
    fn build(&self, ink: &Ink) -> Build;
}

/// The layout model's own preview-grade conversion.
///
/// Correct enough for a swatch and for deciding whether a spot must be
/// converted at all. Not correct enough to print, and specifically not
/// correct enough for UCR, which is documented on
/// [`Ink::to_cmyk`].
#[derive(Debug, Clone, Copy, Default)]
pub struct NaiveBuild;

impl CmykSource for NaiveBuild {
    fn build(&self, ink: &Ink) -> Build {
        ink.to_cmyk()
    }
}

/// A build supplied by the caller, keyed by ink name.
///
/// A prepress provider names a swatch once and the build is fixed for
/// the job, so a name-keyed table is the honest shape: the CMYK on a
/// separator's spec sheet is authoritative, and recomputing it from a
/// screen value would replace their answer with a guess.
#[derive(Debug, Clone, Default)]
pub struct NamedBuilds {
    builds: Vec<(String, Build)>,
    /// Used for any ink with no entry.
    fallback: Build,
}

impl NamedBuilds {
    pub fn new() -> NamedBuilds {
        NamedBuilds {
            builds: Vec::new(),
            fallback: [0.0, 0.0, 0.0, 1.0],
        }
    }

    pub fn with(mut self, name: impl Into<String>, build: Build) -> NamedBuilds {
        self.set(name, build);
        self
    }

    pub fn set(&mut self, name: impl Into<String>, build: Build) {
        let name = name.into();
        match self.builds.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = build,
            None => self.builds.push((name, build)),
        }
    }

    pub fn fallback(mut self, build: Build) -> NamedBuilds {
        self.fallback = build;
        self
    }
}

impl CmykSource for NamedBuilds {
    fn build(&self, ink: &Ink) -> Build {
        self.builds
            .iter()
            .find(|(n, _)| *n == ink.name)
            .map(|(_, b)| *b)
            .unwrap_or(self.fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_naive_build_is_preview_grade() {
        // One channel is always zero, which is why UCR cannot use it.
        let ink = Ink::process("Any", [0.7, 0.5, 0.3]);
        let build = NaiveBuild.build(&ink);
        assert_eq!(build.iter().filter(|v| **v <= 0.0).count(), 1);
    }

    #[test]
    fn a_named_build_overrides_the_naive_one() {
        let source = NamedBuilds::new()
            .with("PANTONE 032 C", [0.0, 0.71, 0.94, 0.0])
            .with("Black", [0.0, 0.0, 0.0, 1.0])
            .fallback([0.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            source.build(&Ink::spot("PANTONE 032 C", [50.0, 60.0, 55.0])),
            [0.0, 0.71, 0.94, 0.0]
        );
        // An unknown ink falls back rather than being dropped.
        assert_eq!(
            source.build(&Ink::spot("UNKNOWN", [1.0, 2.0, 3.0])),
            [0.0, 0.0, 0.0, 1.0]
        );
    }

    #[test]
    fn a_named_build_can_be_replaced_without_duplicating() {
        let mut source = NamedBuilds::new().with("Cyan", [1.0, 0.0, 0.0, 0.0]);
        source.set("Cyan", [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(source.builds.len(), 1);
        assert_eq!(
            source.build(&Ink::process("Cyan", [0.0, 1.0, 0.0])),
            [0.0, 1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn a_supplied_build_can_have_no_zero_channel() {
        // The point of injecting a build: a real separation of a mid tone
        // uses all four inks, which the naive conversion cannot produce.
        let source = NamedBuilds::new().with("Mid tone", [0.62, 0.48, 0.35, 0.18]);
        let build = source.build(&Ink::process("Mid tone", [0.5, 0.5, 0.5]));
        assert!(build.iter().all(|v| *v > 0.0), "{build:?}");
    }
}
