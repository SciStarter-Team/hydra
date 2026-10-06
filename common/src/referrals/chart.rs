//! The referral report's chart: total sign-ups as a line, and above-aligned
//! 100% columns of where the sign-ups with a recorded source came from.
//!
//! **Shared file.** Byte-identical in orrery (`src/staff/referrals/chart.rs`)
//! and hydra (`common/src/referrals/chart.rs`); see `sources.rs`. The drawing
//! itself is the report kit's (`report_kit/svg.rs`).

use super::super::report_kit::format::thousands;
use super::super::report_kit::month::Month;
use super::super::report_kit::svg::{self, BAR_W, Frame};
use super::sources::Group;

/// One month of the chart's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartMonth {
    pub month: Month,
    /// Every sign-up in the month, whether or not its source is known.
    pub signups: i64,
    /// Sign-ups with a known source, per group, indexed by [`Group::index`].
    pub groups: [i64; Group::ALL.len()],
}

impl ChartMonth {
    fn attributed(&self) -> i64 {
        self.groups.iter().sum()
    }

    /// Each group's share of the month's sign-ups with a known source, in
    /// percent; all zero for a month with none.
    fn shares(&self) -> Vec<f64> {
        let total = self.attributed();

        self.groups
            .iter()
            .map(|&n| {
                if total == 0 {
                    0.0
                } else {
                    n as f64 * 100.0 / total as f64
                }
            })
            .collect()
    }
}

const TITLE_ID: &str = "referrals-title";
const LINE_TOP: f64 = 124.0;
const LINE_H: f64 = 110.0;
const BAR_TOP: f64 = 300.0;
const BAR_H: f64 = 300.0;
const LEGEND_TOP: f64 = 668.0;
const LEGEND_COLUMN: f64 = 240.0;
const LEGEND_ROW: f64 = 20.0;
const LEGEND_COLUMNS: usize = 3;

/// Renders the chart for `months`, which must be in order and contiguous.
pub fn render(months: &[ChartMonth]) -> String {
    let frame = Frame::new(months.len());

    let legend_rows = Group::ALL.len().div_ceil(LEGEND_COLUMNS) as f64;
    let foot_top = LEGEND_TOP + legend_rows * LEGEND_ROW + 14.0;
    let height = foot_top + 30.0;

    let range = match (months.first(), months.last()) {
        (Some(first), Some(last)) if first.month == last.month => first.month.long_label(),
        (Some(first), Some(last)) => {
            format!("{} – {}", first.month.long_label(), last.month.long_label())
        }
        _ => String::from("no months"),
    };

    let mut out = String::new();
    svg::open(&mut out, frame.width, height, TITLE_ID);
    svg::title(
        &mut out,
        TITLE_ID,
        "Where new SciStarter sign-ups came from",
        &[
            format!(
                "Monthly sign-ups, {range}, and the referral sources of those with one recorded."
            ),
            String::from(
                "Excludes EvolveMe, Canvas and Clever accounts. CSM = Citizen Science Month.",
            ),
        ],
    );

    // Top panel: total sign-ups on their own count scale.
    svg::panel_heading(&mut out, "Total new accounts", LINE_TOP, 18.0);
    let month_list: Vec<Month> = months.iter().map(|m| m.month).collect();
    let signups: Vec<i64> = months.iter().map(|m| m.signups).collect();
    svg::line_panel(
        &mut out,
        &frame,
        &month_list,
        &signups,
        LINE_TOP,
        LINE_H,
        "sign-ups",
    );

    // Bottom panel: shares by referral source.
    svg::panel_heading(&mut out, "Share by referral source", BAR_TOP, 32.0);

    let by = |value: f64| BAR_TOP + BAR_H - value / 100.0 * BAR_H;
    let ticks: Vec<(f64, String)> = [0, 25, 50, 75, 100]
        .into_iter()
        .map(|tick| (by(f64::from(tick)), format!("{tick}%")))
        .collect();
    svg::gridlines(&mut out, &frame, &ticks);

    for (i, m) in months.iter().enumerate() {
        let x = frame.x_center(i) - BAR_W / 2.0;
        let attributed = m.attributed();

        if attributed > 0 {
            for (k, top, h, is_top) in svg::stacked_segments(&m.shares(), 100.0, BAR_TOP, BAR_H) {
                let group = Group::ALL[k];
                let count = m.groups[k];
                let title = format!(
                    "{} · {}: {} of {} ({:.0}%)",
                    m.month.short_label(),
                    group.label(),
                    thousands(count),
                    thousands(attributed),
                    count as f64 * 100.0 / attributed as f64
                );

                svg::segment(&mut out, x, top, h, is_top, group.color(), &title);
            }
        }

        if m.month.month == 4 {
            out.push_str(&format!(
                r#"<text x="{:.2}" y="{:.2}" text-anchor="middle" class="callout">CSM</text>"#,
                frame.x_center(i),
                BAR_TOP - 8.0
            ));
        }

        svg::month_label(&mut out, &frame, i, m.month, BAR_TOP + BAR_H + 16.0);
    }

    let entries: Vec<(&str, &str)> = Group::ALL
        .into_iter()
        .map(|group| (group.label(), group.color()))
        .collect();
    svg::legend(
        &mut out,
        &entries,
        LEGEND_TOP,
        LEGEND_COLUMNS,
        LEGEND_COLUMN,
        LEGEND_ROW,
    );

    let bases: Vec<i64> = months
        .iter()
        .map(ChartMonth::attributed)
        .filter(|&n| n > 0)
        .collect();
    let base_note = match (bases.iter().min(), bases.iter().max()) {
        (Some(lo), Some(hi)) if lo == hi => format!(" ({} a month)", thousands(*lo)),
        (Some(lo), Some(hi)) => format!(" ({}–{} a month)", thousands(*lo), thousands(*hi)),
        _ => String::new(),
    };

    svg::foot(
        &mut out,
        &[
            format!(
                "Each column is 100% of that month's sign-ups with a recorded referral source{base_note}."
            ),
            String::from("Sign-ups with no recorded source are not shown."),
        ],
        foot_top,
    );

    svg::close(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::super::super::report_kit::format::escape;
    use super::*;

    fn month(y: i32, m: u32, signups: i64, groups: [i64; 9]) -> ChartMonth {
        ChartMonth {
            month: Month::new(y, m).unwrap(),
            signups,
            groups,
        }
    }

    #[test]
    fn a_month_with_no_known_sources_draws_no_column() {
        let svg = render(&[month(2025, 1, 50, [0; 9])]);
        assert!(!svg.contains("<rect x=\"7"));
        assert!(!svg.contains("<path"));
        assert!(svg.contains("Each column is 100%"));
    }

    #[test]
    fn shares_are_of_the_sign_ups_with_a_known_source() {
        let shares = month(2025, 1, 1000, [3, 1, 0, 0, 0, 0, 0, 0, 0]).shares();
        assert_eq!(shares[0], 75.0);
        assert_eq!(shares[1], 25.0);
        assert!(
            month(2025, 1, 10, [0; 9])
                .shares()
                .iter()
                .all(|s| *s == 0.0)
        );
    }

    #[test]
    fn every_april_is_marked_as_citizen_science_month() {
        let months: Vec<ChartMonth> = (1..=12)
            .map(|m| month(2025, m, 10, [10, 0, 0, 0, 0, 0, 0, 0, 0]))
            .collect();
        assert_eq!(render(&months).matches(">CSM<").count(), 1);
    }

    #[test]
    fn the_subtitle_names_the_range() {
        let months = vec![
            month(2025, 11, 10, [10, 0, 0, 0, 0, 0, 0, 0, 0]),
            month(2026, 1, 10, [10, 0, 0, 0, 0, 0, 0, 0, 0]),
        ];
        assert!(render(&months).contains("November 2025 – January 2026"));
    }

    #[test]
    fn the_legend_lists_every_group_even_without_data() {
        let svg = render(&[month(2025, 1, 10, [10, 0, 0, 0, 0, 0, 0, 0, 0])]);
        for group in Group::ALL {
            assert!(
                svg.contains(&format!(">{}<", escape(group.label()))),
                "{group:?}"
            );
        }
    }

    #[test]
    fn segments_carry_a_hover_title() {
        let svg = render(&[month(2025, 9, 2111, [699, 79, 38, 3, 8, 28, 9, 0, 80])]);
        assert!(svg.contains("<title>Sep 2025 · Google search: 699 of 944 (74%)</title>"));
        assert!(svg.contains("<title>Sep 2025: 2,111 sign-ups</title>"));
    }
}
