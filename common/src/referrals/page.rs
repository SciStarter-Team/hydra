//! The referral report's HTML: its page, and the fragment that page loads
//! with the chart and tables.
//!
//! **Shared file.** Byte-identical in orrery (`src/staff/referrals/page.rs`)
//! and hydra (`common/src/referrals/page.rs`); see `sources.rs`. The page
//! shell is the report kit's (`report_kit/shell.rs`).

use std::fmt::Write;

use super::super::report_kit::format::escape;
use super::super::report_kit::month::{DefaultSpan, Range};
use super::super::report_kit::shell::{self, Page};
use super::report::ReportView;

/// Where both sites serve the report.
pub const BASE_URL: &str = "/staff/signup-referrals";

/// How far back the report reaches when no start month is asked for.
pub const DEFAULT_SPAN: DefaultSpan = DefaultSpan::SinceJanuaryLastYear;

const PAGE: Page<'static> = Page {
    title: "Sign-up Referral Sources",
    standfirst: "Where new accounts came from, month by month: total sign-ups, and the share of each referral source among the sign-ups that recorded one. Computed from the database on every load.",
    links: &[("/staff/moore", "Moore grant metrics")],
    base_url: BASE_URL,
};

/// The page: its range controls, and a placeholder that loads the report
/// for `range`.
pub fn shell(range: Range) -> String {
    shell::page(&PAGE, range)
}

/// The report itself: the chart, the top sources of each month, and the
/// chart's numbers as a table.
pub fn fragment(report: &ReportView, range: Range, generated: &str) -> String {
    let mut html = shell::chart_section(
        range,
        generated,
        "signup-referrals",
        &report.chart_svg,
        "Excludes EvolveMe (including accounts that arrived through its task pages without its tag), Canvas, and Clever. Sign-ups with no recorded referrer or tag count in the totals but not in the shares; from March 2026 until October 2026 many sign-ups lost their referrer, so the shares in those months rest on fewer sign-ups.",
    );

    html.push_str(
        r#"<section>
  <h2>Top sources by month <span class="note">ten per month &middot; sign-ups</span></h2>
  <div class="table-scroll">
    <table>
      <thead>
        <tr>
          <th scope="col">Month</th>
          <th scope="col" class="num">Sign-ups</th>
          <th scope="col" class="num">With a source</th>
          <th scope="col">Top sources</th>
        </tr>
      </thead>
      <tbody>
"#,
    );

    for month in &report.months {
        let sources: Vec<String> = month
            .top
            .iter()
            .map(|cell| {
                format!(
                    "<span>{} <b>{}</b></span>",
                    escape(&cell.label),
                    escape(&cell.count)
                )
            })
            .collect();

        let _ = write!(
            html,
            r#"        <tr>
          <th scope="row">{}</th>
          <td class="num">{}</td>
          <td class="num">{}</td>
          <td class="sources">{}</td>
        </tr>
"#,
            escape(&month.label),
            escape(&month.signups),
            escape(&month.attributed),
            sources.join(" &middot; "),
        );
    }

    html.push_str(
        r#"      </tbody>
    </table>
  </div>
</section>

<section>
  <details>
    <summary>Chart data: sign-ups with a known source, by series</summary>
    <div class="table-scroll">
      <table>
        <thead>
          <tr>
            <th scope="col">Month</th>
"#,
    );

    for heading in &report.group_headings {
        let _ = writeln!(
            html,
            r#"            <th scope="col" class="num">{}</th>"#,
            escape(heading)
        );
    }

    html.push_str(
        r#"          </tr>
        </thead>
        <tbody>
"#,
    );

    for row in &report.group_rows {
        let counts: Vec<String> = row
            .counts
            .iter()
            .map(|count| format!(r#"<td class="num">{}</td>"#, escape(count)))
            .collect();

        let _ = writeln!(
            html,
            r#"          <tr><th scope="row">{}</th>{}</tr>"#,
            escape(&row.label),
            counts.join("")
        );
    }

    html.push_str(
        r#"        </tbody>
      </table>
    </div>
  </details>
</section>
"#,
    );

    html
}

#[cfg(test)]
mod tests {
    use super::super::super::report_kit::month::Month;
    use super::super::report::{MonthReport, view};
    use super::super::sources::Source;
    use super::*;

    fn month(year: i32, month: u32) -> Month {
        Month::new(year, month).unwrap()
    }

    fn range(first: Month, last: Month) -> Range {
        Range::resolve(Some(first), Some(last), month(2026, 10), DEFAULT_SPAN)
    }

    #[test]
    fn the_shell_is_the_referral_page() {
        let html = shell(range(month(2025, 1), month(2026, 9)));

        assert!(html.contains("<h1>Sign-up Referral Sources</h1>"));
        assert!(html.contains(r#"action="/staff/signup-referrals""#));
        assert!(
            html.contains(r#"hx-get="/staff/signup-referrals/data?from=2025-01&amp;to=2026-09""#)
        );
        assert!(html.contains(r#"<a href="/staff/moore">Moore grant metrics</a>"#));
    }

    #[test]
    fn the_fragment_renders_the_chart_and_both_tables() {
        let months = vec![
            MonthReport {
                month: month(2025, 4),
                signups: 3,
                sources: vec![(Source::Nasa, 2)],
            },
            MonthReport {
                month: month(2025, 5),
                signups: 1,
                sources: vec![(Source::GoogleSearch, 1)],
            },
        ];
        let html = fragment(&view(&months), range(month(2025, 4), month(2025, 5)), "now");

        assert!(html.contains("<svg"));
        assert!(html.contains("April 2025 – May 2025"));
        assert!(html.contains("<span>NASA <b>2</b></span>"));
        assert!(html.contains(r#"data-filename="signup-referrals-2025-04-to-2025-05.png""#));
        assert!(html.contains(r#"<th scope="col" class="num">LinkedIn</th>"#));
    }

    #[test]
    fn source_names_are_escaped() {
        let months = vec![MonthReport {
            month: month(2025, 4),
            signups: 1,
            sources: vec![(Source::Tagged(String::from("<b>x</b>")), 1)],
        }];
        let html = fragment(&view(&months), range(month(2025, 4), month(2025, 4)), "now");

        assert!(html.contains("&lt;b&gt;x&lt;/b&gt;"));
        assert!(!html.contains("<b>x</b>"));
    }
}
