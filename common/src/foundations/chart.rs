//! The Foundations report's chart: the running total as a line, completions
//! each month as columns split EvolveMe / everyone else, and everyone else
//! alone on its own scale.
//!
//! **Shared file.** Byte-identical in orrery (`src/staff/foundations/chart.rs`)
//! and hydra (`common/src/foundations/chart.rs`); see `report.rs`. The drawing
//! is the report kit's (`report_kit/svg.rs`).
//!
//! The split is the report's point, so the two series take the two most
//! distinct hues of the validated palette, are named in a legend set right
//! under their panel's heading, and stack with everyone else on the baseline,
//! where its smaller counts can be read. Everyone else then gets a panel of
//! its own as well: beside EvolveMe's peaks of several thousand a month, a
//! steady few hundred is a sliver in the stacked panel.

use super::super::report_kit::format::thousands;
use super::super::report_kit::month::Month;
use super::super::report_kit::svg::{self, BAR_W, Frame};
use super::report::MonthReport;

/// Everyone else: categorical slot 1.
pub const OTHERS_COLOR: &str = "#2a78d6";
/// EvolveMe: categorical slot 2, the most distinct from slot 1.
pub const EVOLVEME_COLOR: &str = "#eb6834";

const TITLE_ID: &str = "foundations-title";
const LINE_TOP: f64 = 124.0;
const LINE_H: f64 = 100.0;
const STACK_TOP: f64 = 300.0;
const STACK_H: f64 = 220.0;
const OTHERS_TOP: f64 = 580.0;
const OTHERS_H: f64 = 120.0;
const AXIS_TOP: f64 = OTHERS_TOP + OTHERS_H + 16.0;
const FOOT_TOP: f64 = AXIS_TOP + 48.0;
const HEIGHT: f64 = FOOT_TOP + 30.0;

/// Renders the chart for `months`, which must be in order and contiguous.
pub fn render(months: &[MonthReport]) -> String {
    let frame = Frame::new(months.len());

    let range = match (months.first(), months.last()) {
        (Some(first), Some(last)) if first.month == last.month => first.month.long_label(),
        (Some(first), Some(last)) => {
            format!("{} – {}", first.month.long_label(), last.month.long_label())
        }
        _ => String::from("no months"),
    };

    let mut out = String::new();
    svg::open(&mut out, frame.width, HEIGHT, TITLE_ID);
    svg::title(
        &mut out,
        TITLE_ID,
        "Foundations of Citizen Science training completions",
        &[
            format!("People completing the training each month, {range}."),
            String::from(
                "Each person is counted once, when they first completed it; the badge is not required.",
            ),
        ],
    );

    let month_list: Vec<Month> = months.iter().map(|m| m.month).collect();

    // Panel 1: the running total, as the training page's counter shows it.
    svg::panel_heading(&mut out, "Completed to date", LINE_TOP, 18.0);
    let to_date: Vec<i64> = months.iter().map(|m| m.to_date).collect();
    svg::line_panel(
        &mut out,
        &frame,
        &month_list,
        &to_date,
        LINE_TOP,
        LINE_H,
        "completed by then",
    );

    // Panel 2: completions each month, everyone else on the baseline and
    // EvolveMe above.
    svg::panel_heading(&mut out, "Completions each month", STACK_TOP, 50.0);
    svg::legend(
        &mut out,
        &[
            ("Everyone else", OTHERS_COLOR),
            ("EvolveMe", EVOLVEME_COLOR),
        ],
        STACK_TOP - 26.0,
        2,
        160.0,
        20.0,
    );

    let stack_max = months.iter().map(MonthReport::completed).max().unwrap_or(0) as f64;
    let stack_scale = svg::count_gridlines(&mut out, &frame, STACK_TOP, STACK_H, stack_max);

    for (i, m) in months.iter().enumerate() {
        let x = frame.x_center(i) - BAR_W / 2.0;
        let values = [m.others as f64, m.evolveme as f64];

        for (k, top, h, is_top) in svg::stacked_segments(&values, stack_scale, STACK_TOP, STACK_H) {
            let (label, count, color) = if k == 0 {
                ("Everyone else", m.others, OTHERS_COLOR)
            } else {
                ("EvolveMe", m.evolveme, EVOLVEME_COLOR)
            };
            let title = format!(
                "{} · {label}: {} of {}",
                m.month.short_label(),
                thousands(count),
                thousands(m.completed())
            );

            svg::segment(&mut out, x, top, h, is_top, color, &title);
        }
    }

    // Panel 3: everyone else alone, on a scale of its own.
    svg::panel_heading(
        &mut out,
        "Everyone else, on its own scale",
        OTHERS_TOP,
        18.0,
    );

    let others_max = months.iter().map(|m| m.others).max().unwrap_or(0) as f64;
    let others_scale = svg::count_gridlines(&mut out, &frame, OTHERS_TOP, OTHERS_H, others_max);

    for (i, m) in months.iter().enumerate() {
        let x = frame.x_center(i) - BAR_W / 2.0;

        for (_, top, h, is_top) in
            svg::stacked_segments(&[m.others as f64], others_scale, OTHERS_TOP, OTHERS_H)
        {
            let title = format!(
                "{} · Everyone else: {}",
                m.month.short_label(),
                thousands(m.others)
            );
            svg::segment(&mut out, x, top, h, is_top, OTHERS_COLOR, &title);
        }

        svg::month_label(&mut out, &frame, i, m.month, AXIS_TOP);
    }

    svg::foot(
        &mut out,
        &[
            String::from(
                "EvolveMe: accounts from the EvolveMe rewards platform, by tag, origin, or a referrer on its task pages.",
            ),
            String::from(
                "Completed to date is the running total the training page's counter shows.",
            ),
        ],
        FOOT_TOP,
    );

    svg::close(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(m: u32, evolveme: i64, others: i64, to_date: i64) -> MonthReport {
        MonthReport {
            month: Month::new(2025, m).unwrap(),
            evolveme,
            others,
            to_date,
        }
    }

    #[test]
    fn both_series_are_named_and_colored_apart() {
        let svg = render(&[
            report(8, 7_758, 906, 72_222),
            report(9, 4_876, 1_051, 78_149),
        ]);

        assert!(svg.contains(">Everyone else<"));
        assert!(svg.contains(">EvolveMe<"));
        assert!(svg.contains(&format!(r#"fill="{OTHERS_COLOR}""#)));
        assert!(svg.contains(&format!(r#"fill="{EVOLVEME_COLOR}""#)));
        assert_ne!(OTHERS_COLOR, EVOLVEME_COLOR);
    }

    #[test]
    fn every_segment_says_what_it_is_on_hover() {
        let svg = render(&[report(8, 7_758, 906, 72_222)]);

        assert!(svg.contains("<title>Aug 2025 · EvolveMe: 7,758 of 8,664</title>"));
        assert!(svg.contains("<title>Aug 2025 · Everyone else: 906 of 8,664</title>"));
        assert!(svg.contains("<title>Aug 2025 · Everyone else: 906</title>"));
        assert!(svg.contains("<title>Aug 2025: 72,222 completed by then</title>"));
    }

    #[test]
    fn a_month_with_no_completions_draws_no_columns() {
        let svg = render(&[report(8, 0, 0, 100)]);
        assert!(!svg.contains("<title>Aug 2025 · "));
        assert!(!svg.contains("<path"));
    }
}
