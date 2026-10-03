//! What a prepress provider would want to know about a page.
//!
//! Preflight is not decoration. The failure modes it reports -- a missing
//! link or an ink limit exceeded -- are the ones that cost money when
//! they reach a press, and both
//! of them are invisible on screen.

use schist_layout::Pt;

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth knowing, will not stop the job.
    Info,
    /// Likely to be corrected by the designer.
    Warning,
    /// The job will be rejected or will print wrong.
    Error,
}

/// One finding.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
}

/// Everything found on one page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreflightReport {
    pub page: usize,
    pub findings: Vec<Finding>,
    /// The greatest total area coverage anywhere on the page, as a
    /// fraction: 4.0 means 400% ink.
    pub peak_total_area: f32,
    /// The manager's limit, if one is set.
    pub total_area_limit: Option<Pt>,
    /// How many pixels are over that limit.
    pub pixels_over_limit: usize,
}

impl PreflightReport {
    pub fn new(page: usize) -> PreflightReport {
        PreflightReport {
            page,
            ..PreflightReport::default()
        }
    }

    pub fn add(&mut self, severity: Severity, message: impl Into<String>) {
        self.findings.push(Finding {
            severity,
            message: message.into(),
        });
    }

    pub fn missing_link(&mut self, path: &str) {
        self.add(
            Severity::Error,
            schist_i18n::tf!("design.preflight_missing_link", path = path),
        );
    }

    /// A link may exist while its pixels cannot be decoded or fetched.
    /// This still prevents a complete check, including for embedded art.
    pub fn unavailable_graphic(&mut self, path: &str) {
        self.add(
            Severity::Error,
            schist_i18n::tf!("design.preflight_unavailable_graphic", path = path),
        );
    }

    pub fn note_total_area(&mut self, peak: f32, limit: Option<Pt>, over: usize) {
        self.peak_total_area = peak;
        self.total_area_limit = limit;
        self.pixels_over_limit = over;
        match limit {
            // No limit configured means the press's own, which
            // preflight cannot know. Staying quiet is the honest choice;
            // guessing 300% would cry wolf on every dark image.
            None => {}
            Some(limit) if over > 0 => {
                let percent = (peak * 100.0).round() as i32;
                let limit_percent = (limit * 100.0).round() as i32;
                self.add(
                    Severity::Error,
                    schist_i18n::tf!(
                        "design.preflight_coverage_exceeded",
                        percent = percent,
                        limit = limit_percent,
                        pixels = over,
                    ),
                );
            }
            Some(_) => {
                let percent = (peak * 100.0).round() as i32;
                self.add(
                    Severity::Info,
                    schist_i18n::tf!("design.preflight_coverage_peak", percent = percent),
                );
            }
        }
    }

    pub fn errors(&self) -> usize {
        self.count(Severity::Error)
    }

    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }

    /// Whether these checks found no errors. This report currently covers
    /// layout content, graphic availability and total ink coverage, not every press rule.
    pub fn is_printable(&self) -> bool {
        self.errors() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_report_is_printable() {
        let report = PreflightReport::new(0);
        assert!(report.is_printable());
        assert_eq!(report.errors(), 0);
    }

    #[test]
    fn an_error_stops_the_job() {
        let mut report = PreflightReport::new(0);
        report.add(Severity::Error, "linked file is missing: /tmp/a.psd");
        assert!(!report.is_printable());
        assert_eq!(report.errors(), 1);
    }

    #[test]
    fn a_warning_does_not_stop_the_job() {
        let mut report = PreflightReport::new(0);
        report.add(Severity::Warning, "text is set in a process mix");
        assert!(report.is_printable());
        assert_eq!(report.warnings(), 1);
    }

    #[test]
    fn an_ink_limit_breach_is_an_error_with_the_numbers() {
        let mut report = PreflightReport::new(2);
        report.note_total_area(3.4, Some(3.0), 512);
        assert!(!report.is_printable());
        let message = &report.findings[0].message;
        assert!(message.contains("340%"), "{message}");
        assert!(message.contains("512"), "{message}");
        assert!(message.contains("300%"), "{message}");
    }

    #[test]
    fn coverage_within_the_limit_is_only_informational() {
        let mut report = PreflightReport::new(0);
        report.note_total_area(2.4, Some(3.0), 0);
        assert!(report.is_printable());
        assert_eq!(report.findings[0].severity, Severity::Info);
    }

    #[test]
    fn no_limit_configured_stays_quiet() {
        // Guessing a limit would cry wolf on every dark image, and the
        // press's own limit is not something preflight can know.
        let mut report = PreflightReport::new(0);
        report.note_total_area(4.0, None, 1000);
        assert!(report.findings.is_empty());
        assert!(report.is_printable());
    }

    #[test]
    fn severities_order_so_the_worst_can_be_shown_first() {
        assert!(Severity::Error > Severity::Warning);
        assert!(Severity::Warning > Severity::Info);
    }
}
