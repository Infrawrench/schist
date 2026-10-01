//! Design-only preflight: findings from separation, with an explicit
//! preview resolution and a snapshot that becomes stale after edits.

use crate::design::preflight::{check, PREVIEW_DPI};
use schist_separation::{Finding, PreflightReport, Severity};

use super::*;

/// No panel outside Design Mode or without a page to check. A page with
/// no findings still has a useful result, distinct from an unchecked page.
pub(super) fn preflight_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    if !ws.design_mode() || !ws.design.ready() {
        return None;
    }
    let page = ws.design.current_page();
    let document = &ws.design.document;
    document.pages.get(page)?;
    let state = &ws.design.preflight;
    let report = state.report(document, page);
    let status = if state.running {
        t("design.preflight_running").to_string()
    } else if state.stale(document, page) {
        t("design.preflight_stale").to_string()
    } else if let Some(report) = report {
        if report.findings.is_empty() {
            t("design.preflight_no_findings").to_string()
        } else {
            schist_i18n::tf!(
                "design.preflight_counts",
                errors = report.errors(),
                warnings = report.warnings()
            )
        }
    } else {
        t("design.preflight_not_run").to_string()
    };
    let rows = report.map(finding_rows).unwrap_or_default();
    Some(
        div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_h(px(0.0))
            .p_2()
            .gap_2()
            .border_t_1()
            .border_color(gpui::rgb(palette().panel_edge))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .text_color(gpui::rgb(palette().text_dim))
                    .child(schist_i18n::tf!(
                        "design.link_on_page",
                        number = document.page_number(page)
                    ))
                    .child(
                        IconButton::new("design-preflight-run", "refresh")
                            .tooltip(t("design.preflight_run"), None)
                            .disabled(state.running)
                            .on_click(cx.listener(|ws, _, _, cx| ws.check_design_page(cx))),
                    ),
            )
            .child(div().text_xs().child(status))
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(palette().text_dim))
                    .child(schist_i18n::tf!(
                        "design.preflight_preview",
                        dpi = PREVIEW_DPI
                    )),
            )
            .children(rows)
            .into_any_element(),
    )
}

fn ordered_findings(report: &PreflightReport) -> Vec<&Finding> {
    let mut findings: Vec<_> = report.findings.iter().collect();
    findings.sort_by_key(|finding| std::cmp::Reverse(finding.severity));
    findings
}

fn finding_rows(report: &PreflightReport) -> Vec<gpui::AnyElement> {
    ordered_findings(report)
        .into_iter()
        .map(|finding| {
            let label = match finding.severity {
                Severity::Error => "design.preflight_error",
                Severity::Warning => "design.preflight_warning",
                Severity::Info => "design.preflight_info",
            };
            div()
                .flex()
                .flex_col()
                .gap_1()
                .p_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(if finding.severity == Severity::Info {
                            palette().text_dim
                        } else {
                            palette().warning
                        }))
                        .child(t(label)),
                )
                .child(div().text_sm().child(finding.message.clone()))
                .into_any_element()
        })
        .collect()
}

impl Workspace {
    fn check_design_page(&mut self, cx: &mut Context<Self>) {
        if !self.design_mode() {
            return;
        }
        let page = self.design.current_page();
        let Some(request) =
            self.design
                .preflight
                .start(&self.design.document, page, self.design.graphics.clone())
        else {
            return;
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let snapshot = request.clone();
            let report = cx
                .background_executor()
                .spawn(async move { check(&snapshot) })
                .await;
            this.update(cx, |ws, cx| {
                ws.design.preflight.finish(&request, report);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_are_worst_first_and_stable_within_each_severity() {
        let mut report = PreflightReport::new(0);
        for severity in [
            Severity::Info,
            Severity::Error,
            Severity::Warning,
            Severity::Error,
            Severity::Info,
        ] {
            report.add(severity, report.findings.len().to_string());
        }
        let ordered = ordered_findings(&report);
        assert!(ordered
            .windows(2)
            .all(|pair| pair[0].severity >= pair[1].severity));
        for severity in [Severity::Info, Severity::Warning, Severity::Error] {
            assert_eq!(
                ordered
                    .iter()
                    .filter(|f| f.severity == severity)
                    .map(|f| &f.message)
                    .collect::<Vec<_>>(),
                report
                    .findings
                    .iter()
                    .filter(|f| f.severity == severity)
                    .map(|f| &f.message)
                    .collect::<Vec<_>>()
            );
        }
    }
}
