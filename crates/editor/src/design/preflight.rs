//! Cached, on-demand preflight for the page being viewed.
//!
//! Separation allocates plates and rasterises text. It belongs on the
//! background executor, never in a panel's render function. A snapshot
//! also prevents an old report from being presented as a check of an edit.

use std::sync::Arc;

use schist_layout::LayoutDocument;
use schist_separation::{separate_page, OutputSettings, PreflightReport};

/// Interactive coverage is a preview, not an output-resolution proof.
pub const PREVIEW_DPI: f32 = 72.0;

pub struct Request {
    pub document: LayoutDocument,
    pub page: usize,
    pub graphics: Arc<super::graphics::Graphics>,
}

#[derive(Default)]
pub struct PreflightState {
    request: Option<Arc<Request>>,
    report: Option<PreflightReport>,
    pub running: bool,
}

impl PreflightState {
    pub fn start(
        &mut self,
        document: &LayoutDocument,
        page: usize,
        graphics: Arc<super::graphics::Graphics>,
    ) -> Option<Arc<Request>> {
        if self.running || page >= document.pages.len() {
            return None;
        }
        let request = Arc::new(Request {
            document: document.clone(),
            page,
            graphics,
        });
        self.request = Some(request.clone());
        self.report = None;
        self.running = true;
        Some(request)
    }

    /// Identity, rather than matching page numbers, rejects work from a
    /// document closed or replaced while its worker was still running.
    pub fn finish(&mut self, request: &Arc<Request>, report: Option<PreflightReport>) {
        if self
            .request
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, request))
        {
            self.report = report;
            self.running = false;
        }
    }

    pub fn stale(&self, document: &LayoutDocument, page: usize) -> bool {
        self.request
            .as_ref()
            .is_some_and(|request| request.page != page || request.document != *document)
    }

    pub fn report(&self, document: &LayoutDocument, page: usize) -> Option<&PreflightReport> {
        if self.stale(document, page) {
            None
        } else {
            self.report.as_ref()
        }
    }
}

/// Uses the same check as separation with a snapshot of decoded graphics.
/// Unsupported embedded payloads remain unavailable and produce an error.
pub fn check(request: &Request) -> Option<PreflightReport> {
    let mut document = request.document.clone();
    let page = document.pages.get(request.page)?;
    let channels = schist_separation::PlatePlan::build(&document.all_inks(), &document.ink_manager)
        .plates
        .len()
        + 4;
    let width = f64::from(page.bleed_rect().width);
    let height = f64::from(page.bleed_rect().height);
    if !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || width * height * channels as f64 * 4.0 > 512.0 * 1024.0 * 1024.0
    {
        let mut report = PreflightReport::new(request.page);
        report.add(
            schist_separation::Severity::Error,
            schist_i18n::t("design.output_too_large"),
        );
        return Some(report);
    }
    for object in document.objects.iter_mut().chain(
        document
            .parents
            .iter_mut()
            .flat_map(|p| p.objects.iter_mut().map(|o| &mut o.object)),
    ) {
        if let schist_layout::LayoutObject::GraphicFrame {
            link,
            embedded: false,
            ..
        } = &mut object.object
        {
            if request.graphics.get(&link.path).is_some() {
                link.present = true;
            }
        }
    }
    separate_page(
        &document,
        request.page,
        OutputSettings::at(PREVIEW_DPI),
        request.graphics.as_ref(),
    )
    .map(|page| page.report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::{blank_a4, Page};

    #[test]
    fn results_belong_to_the_checked_document_and_page() {
        let mut document = blank_a4();
        document.add_page(Page::a4());
        let mut state = PreflightState::default();
        let request = state.start(&document, 0, Default::default()).unwrap();
        state.finish(&request, Some(PreflightReport::new(0)));
        assert!(state.report(&document, 0).is_some());
        assert!(state.report(&document, 1).is_none());
        // Edits outside the active page can change inherited artwork or
        // inks. A page-number-only cache would incorrectly stay current.
        document.ink_manager.total_area_limit = Some(2.0);
        assert!(state.report(&document, 0).is_none());
        assert!(state.stale(&document, 0));
        document = request.document.clone();
        assert!(
            state.report(&document, 0).is_some(),
            "restoring the exact snapshot restores its report"
        );
    }

    #[test]
    fn an_old_worker_cannot_finish_a_new_documents_check() {
        let document = blank_a4();
        let mut state = PreflightState::default();
        let old = state.start(&document, 0, Default::default()).unwrap();
        state = PreflightState::default();
        let new = state.start(&document, 0, Default::default()).unwrap();
        state.finish(&old, Some(PreflightReport::new(0)));
        assert!(state.running);
        assert!(state.report(&document, 0).is_none());
        state.finish(&new, Some(PreflightReport::new(0)));
        assert!(!state.running);
        assert!(state.report(&document, 0).is_some());
    }

    #[test]
    fn invalid_pages_and_repeated_clicks_do_not_start_work() {
        let document = blank_a4();
        let mut state = PreflightState::default();
        assert!(state
            .start(&document, document.pages.len(), Default::default())
            .is_none());
        assert!(!state.running);
        let request = state.start(&document, 0, Default::default()).unwrap();
        assert!(state.start(&document, 0, Default::default()).is_none());
        state.finish(&request, None);
        assert!(state.start(&document, 0, Default::default()).is_some());
    }
}
