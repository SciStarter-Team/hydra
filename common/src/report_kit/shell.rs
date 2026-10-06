//! The page a monthly report is served in (`shell.html`), and the chart
//! section its data fragment opens with. **Shared file**; see
//! `report_kit.rs`.
//!
//! The markup lives here rather than in either site's templates because the
//! two sites' template engines disagree on how a template names its fields,
//! and the pages should not be written twice.

use std::fmt::Write;

use super::format::escape;
use super::month::Range;

const SHELL: &str = include_str!("shell.html");

/// What distinguishes one report's page from another's.
pub struct Page<'a> {
    pub title: &'a str,
    /// The paragraph under the title.
    pub standfirst: &'a str,
    /// Links to related staff pages, as `(href, label)`, in the toolbar.
    pub links: &'a [(&'a str, &'a str)],
    /// The report's own path; its data fragment is served at `/data` below.
    pub base_url: &'a str,
}

/// The page: its range controls, and a placeholder that loads the report for
/// `range` from `{base_url}/data`.
pub fn page(page: &Page<'_>, range: Range) -> String {
    let links: Vec<String> = page
        .links
        .iter()
        .map(|(href, label)| format!(r#"<a href="{}">{}</a>"#, escape(href), escape(label)))
        .collect();

    SHELL
        .replace("{{title}}", &escape(page.title))
        .replace("{{standfirst}}", &escape(page.standfirst))
        .replace("{{links}}", &links.join("\n          "))
        .replace("{{base_url}}", &escape(page.base_url))
        .replace("{{query}}", &escape(&range.query_string()))
        .replace("{{from}}", &range.first.to_string())
        .replace("{{to}}", &range.last.to_string())
}

/// The fragment's opening section: the range, when it was computed, the
/// "Download PNG" button the shell's script answers, the chart, and a note.
pub fn chart_section(
    range: Range,
    generated: &str,
    file_stem: &str,
    chart_svg: &str,
    note: &str,
) -> String {
    let mut html = String::new();

    let _ = write!(
        html,
        r#"<section>
  <h2>
    {range}
    <span class="note">as of {generated}</span>
    <button type="button" class="download" data-download-png data-filename="{file}.png">Download PNG</button>
  </h2>
  <div class="card chart-card" id="report-chart">
    {chart_svg}
  </div>
  <p class="prose">{note}</p>
</section>
"#,
        range = escape(&range.label()),
        generated = escape(generated),
        file = escape(&range.file_stem(file_stem)),
        note = escape(note),
    );

    html
}

#[cfg(test)]
mod tests {
    use super::super::month::{DefaultSpan, Month};
    use super::*;

    fn range() -> Range {
        Range::resolve(
            Month::new(2025, 1),
            Month::new(2026, 9),
            Month::new(2026, 10).unwrap(),
            DefaultSpan::Trailing(24),
        )
    }

    #[test]
    fn the_page_carries_its_range_into_the_controls_and_the_loader() {
        let html = page(
            &Page {
                title: "A Report",
                standfirst: "What it shows & why.",
                links: &[("/staff/moore", "Moore grant metrics")],
                base_url: "/staff/a-report",
            },
            range(),
        );

        assert!(html.contains("<title>A Report</title>"));
        assert!(html.contains("<h1>A Report</h1>"));
        assert!(html.contains("What it shows &amp; why."));
        assert!(html.contains(r#"<a href="/staff/moore">Moore grant metrics</a>"#));
        assert!(html.contains(r#"action="/staff/a-report""#));
        assert!(html.contains(r#"name="from" value="2025-01""#));
        assert!(html.contains(r#"name="to" value="2026-09""#));
        assert!(html.contains(r#"hx-get="/staff/a-report/data?from=2025-01&amp;to=2026-09""#));
        assert!(!html.contains("{{"));
    }

    #[test]
    fn the_chart_section_names_its_range_and_download() {
        let html = chart_section(range(), "now", "a-report", "<svg></svg>", "Note <b>.");

        assert!(html.contains("January 2025 – September 2026"));
        assert!(html.contains(r#"data-filename="a-report-2025-01-to-2026-09.png""#));
        assert!(html.contains(r#"id="report-chart""#));
        assert!(html.contains("<svg></svg>"));
        assert!(html.contains("Note &lt;b&gt;."));
    }
}
