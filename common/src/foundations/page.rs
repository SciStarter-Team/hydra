//! The Foundations report's HTML: its page, and the fragment that page loads
//! with the chart and table.
//!
//! **Shared file.** Byte-identical in orrery (`src/staff/foundations/page.rs`)
//! and hydra (`common/src/foundations/page.rs`); see `report.rs`. The page
//! shell is the report kit's (`report_kit/shell.rs`).

use std::fmt::Write;

use super::super::report_kit::format::escape;
use super::super::report_kit::month::{DefaultSpan, Range};
use super::super::report_kit::shell::{self, Page};
use super::report::ReportView;

/// Where both sites serve the report.
pub const BASE_URL: &str = "/staff/foundations-completions";

/// How far back the report reaches when no start month is asked for.
pub const DEFAULT_SPAN: DefaultSpan = DefaultSpan::Trailing(24);

const PAGE: Page<'static> = Page {
    title: "Foundations Training Completions",
    standfirst: "How many people complete the Foundations of Citizen Science training each month, how many of them came through EvolveMe, and the running total the training page shows. Computed from the database on every load.",
    links: &[
        ("/staff/moore", "Moore grant metrics"),
        ("/staff/signup-referrals", "Sign-up referral sources"),
    ],
    base_url: BASE_URL,
};

/// The page: its range controls, and a placeholder that loads the report
/// for `range`.
pub fn shell(range: Range) -> String {
    shell::page(&PAGE, range)
}

/// The report itself: the chart, and its numbers as a table.
pub fn fragment(report: &ReportView, range: Range, generated: &str) -> String {
    let mut html = shell::chart_section(
        range,
        generated,
        "foundations-completions",
        &report.chart_svg,
        &format!(
            "{} people have completed the training to date. Each is counted once, in the month of the earliest evidence that they finished the module: its completion record, the badge, or the step after it. EvolveMe accounts are those tagged at signup, carrying its origin, or arriving from one of its task pages.",
            report.completed_to_date
        ),
    );

    html.push_str(
        r#"<section>
  <h2>Month by month <span class="note">people completing the training</span></h2>
  <div class="table-scroll">
    <table>
      <thead>
        <tr>
          <th scope="col">Month</th>
          <th scope="col" class="num">Completed</th>
          <th scope="col" class="num">EvolveMe</th>
          <th scope="col" class="num">Everyone else</th>
          <th scope="col" class="num">Completed to date</th>
        </tr>
      </thead>
      <tbody>
"#,
    );

    for row in &report.rows {
        let _ = write!(
            html,
            r#"        <tr>
          <th scope="row">{}</th>
          <td class="num">{}</td>
          <td class="num">{}</td>
          <td class="num">{}</td>
          <td class="num">{}</td>
        </tr>
"#,
            escape(&row.label),
            escape(&row.completed),
            escape(&row.evolveme),
            escape(&row.others),
            escape(&row.to_date),
        );
    }

    html.push_str(
        r#"      </tbody>
    </table>
  </div>
</section>
"#,
    );

    html
}

#[cfg(test)]
mod tests {
    use super::super::super::report_kit::month::Month;
    use super::super::report::{MonthCount, tally, view};
    use super::*;

    fn month(year: i32, month: u32) -> Month {
        Month::new(year, month).unwrap()
    }

    #[test]
    fn the_default_is_the_last_two_years() {
        let range = Range::resolve(None, None, month(2026, 10), DEFAULT_SPAN);
        assert_eq!(range.first, month(2024, 10));
        assert_eq!(range.last, month(2026, 9));
    }

    #[test]
    fn the_shell_is_the_foundations_page() {
        let range = Range::resolve(None, None, month(2026, 10), DEFAULT_SPAN);
        let html = shell(range);

        assert!(html.contains("<h1>Foundations Training Completions</h1>"));
        assert!(html.contains(
            r#"hx-get="/staff/foundations-completions/data?from=2024-10&amp;to=2026-09""#
        ));
        assert!(html.contains(r#"<a href="/staff/signup-referrals">Sign-up referral sources</a>"#));
    }

    #[test]
    fn the_fragment_renders_the_chart_and_table() {
        let range = Range::resolve(
            Some(month(2026, 9)),
            Some(month(2026, 9)),
            month(2026, 10),
            DEFAULT_SPAN,
        );
        let counts = [MonthCount {
            month: month(2026, 9),
            evolveme: 27,
            others: 516,
        }];
        let html = fragment(&view(&tally(range, &counts), 92_803), range, "now");

        assert!(html.contains("<svg"));
        assert!(html.contains("92,803 people have completed the training to date."));
        assert!(html.contains(r#"<th scope="row">Sep 2026</th>"#));
        assert!(html.contains(r#"<td class="num">516</td>"#));
        assert!(html.contains(r#"data-filename="foundations-completions-2026-09-to-2026-09.png""#));
    }
}
