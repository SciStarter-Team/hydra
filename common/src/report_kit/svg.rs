//! Drawing the reports' charts: one self-contained SVG per chart — title,
//! panels, legend and notes, with its styles inline — so a page can rasterize
//! it to a PNG for email exactly as it is shown. **Shared file**; see
//! `report_kit.rs`.
//!
//! A chart is a stack of panels over one month axis. Panels never share a
//! y-axis: a count and a share, or two counts of different sizes, have
//! unrelated scales, and overlaying them invents crossings that mean nothing.
//!
//! The marks follow the reports' chart conventions: 20px columns with a 2px
//! surface gap between stacked segments and a rounded top, a 2px line, round
//! gridline steps, hover titles on every mark, and text in ink tokens, never
//! a series color.

use std::fmt::Write;

use super::format::{escape, thousands};
use super::month::Month;

pub const SURFACE: &str = "#fcfcfb";
pub const INK: &str = "#0b0b0b";
pub const INK2: &str = "#52514e";
pub const MUTED: &str = "#898781";
pub const GRID: &str = "#e1e0d9";
pub const AXIS: &str = "#c3c2b7";
const FONT: &str = r#"system-ui, -apple-system, "Segoe UI", sans-serif"#;

const MIN_WIDTH: f64 = 720.0;
const MIN_SLOT: f64 = 30.0;
pub const PLOT_LEFT: f64 = 56.0;
const PLOT_RIGHT_PAD: f64 = 20.0;
pub const BAR_W: f64 = 20.0;
const GAP: f64 = 2.0;
const RADIUS: f64 = 4.0;

/// The horizontal geometry of a chart over `months` months: a long range
/// widens the chart rather than squeezing its columns.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub width: f64,
    pub plot_right: f64,
    slot: f64,
}

impl Frame {
    pub fn new(months: usize) -> Frame {
        let n = months.max(1) as f64;
        let width = MIN_WIDTH.max(PLOT_LEFT + PLOT_RIGHT_PAD + n * MIN_SLOT);
        let plot_right = width - PLOT_RIGHT_PAD;

        Frame {
            width,
            plot_right,
            slot: (plot_right - PLOT_LEFT) / n,
        }
    }

    /// The horizontal center of the `i`th month.
    pub fn x_center(&self, i: usize) -> f64 {
        PLOT_LEFT + self.slot * (i as f64 + 0.5)
    }
}

/// A round gridline step giving about two or three lines under `max`.
pub fn nice_step(max: f64) -> f64 {
    if max <= 0.0 {
        return 1.0;
    }

    let rough = max / 2.5;
    let magnitude = 10f64.powf(rough.log10().floor());

    for step in [1.0, 2.0, 2.5, 5.0, 10.0] {
        if rough <= step * magnitude {
            return step * magnitude;
        }
    }

    10.0 * magnitude
}

/// A column segment with rounded top corners and a square base.
fn rounded_top(x: f64, top: f64, w: f64, h: f64, r: f64) -> String {
    let r = r.min(h).min(w / 2.0);
    let bottom = top + h;

    format!(
        "M{x:.2},{bottom:.2} L{x:.2},{tr:.2} Q{x:.2},{top:.2} {xr:.2},{top:.2} \
         L{xwr:.2},{top:.2} Q{xw:.2},{top:.2} {xw:.2},{tr:.2} L{xw:.2},{bottom:.2} Z",
        tr = top + r,
        xr = x + r,
        xwr = x + w - r,
        xw = x + w,
    )
}

/// Opens the SVG, with its styles and background. `title_id` names the
/// element [`title`] writes, which labels the image for assistive tech.
pub fn open(svg: &mut String, width: f64, height: f64, title_id: &str) {
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-labelledby="{title_id}">"#
    );
    let _ = write!(
        svg,
        r#"<style>
text {{ font-family: {FONT}; }}
.title {{ font-size: 17px; font-weight: 600; fill: {INK}; }}
.sub {{ font-size: 13px; fill: {INK2}; }}
.panel {{ font-size: 13px; font-weight: 600; fill: {INK}; }}
.tick {{ font-size: 11px; fill: {MUTED}; font-variant-numeric: tabular-nums; }}
.year {{ font-size: 11px; font-weight: 600; fill: {INK2}; }}
.callout {{ font-size: 11px; font-weight: 600; fill: {INK2}; }}
.value {{ font-size: 11px; font-weight: 600; fill: {INK}; }}
.legend {{ font-size: 12px; fill: {INK2}; }}
.foot {{ font-size: 11px; fill: {MUTED}; }}
</style>"#
    );
    let _ = write!(
        svg,
        r#"<rect width="{width}" height="{height}" fill="{SURFACE}"/>"#
    );
}

/// The chart's title and subtitle lines.
pub fn title(svg: &mut String, title_id: &str, title: &str, subtitles: &[String]) {
    let _ = write!(
        svg,
        r#"<text id="{title_id}" x="20" y="30" class="title">{}</text>"#,
        escape(title)
    );

    for (i, line) in subtitles.iter().enumerate() {
        let _ = write!(
            svg,
            r#"<text x="20" y="{}" class="sub">{}</text>"#,
            54 + 18 * i,
            escape(line)
        );
    }
}

/// A panel's heading, `gap` above the panel's top.
pub fn panel_heading(svg: &mut String, text: &str, panel_top: f64, gap: f64) {
    let _ = write!(
        svg,
        r#"<text x="20" y="{:.0}" class="panel">{}</text>"#,
        panel_top - gap,
        escape(text)
    );
}

/// Horizontal gridlines at the given heights, each with its label; the
/// first is the baseline, drawn darker.
pub fn gridlines(svg: &mut String, frame: &Frame, lines: &[(f64, String)]) {
    for (index, (y, label)) in lines.iter().enumerate() {
        let colour = if index == 0 { AXIS } else { GRID };
        let _ = write!(
            svg,
            r#"<line x1="{PLOT_LEFT}" x2="{:.2}" y1="{y:.2}" y2="{y:.2}" stroke="{colour}" stroke-width="1"/><text x="{:.0}" y="{:.2}" text-anchor="end" class="tick">{}</text>"#,
            frame.plot_right,
            PLOT_LEFT - 8.0,
            y + 4.0,
            label
        );
    }
}

/// Gridlines for a count scale over `0..=max` drawn in a panel `height`
/// tall from `top`: round steps up to the largest value, with headroom above
/// it for a label. Returns the scale's top value.
pub fn count_gridlines(svg: &mut String, frame: &Frame, top: f64, height: f64, max: f64) -> f64 {
    let step = nice_step(max);
    let scale = (max * 1.15).max(step);
    let y = |value: f64| top + height - value / scale * height;

    let mut lines = Vec::new();
    let mut tick = 0.0;
    while tick <= max.max(0.0) + 0.5 {
        lines.push((y(tick), thousands(tick as i64)));
        tick += step;
    }

    gridlines(svg, frame, &lines);
    scale
}

/// A line panel: one count per month on its own scale, with every month
/// hoverable and the peak, the low and the latest month labelled. `noun`
/// completes each month's hover title: "Sep 2025: 2,111 sign-ups".
pub fn line_panel(
    svg: &mut String,
    frame: &Frame,
    months: &[Month],
    values: &[i64],
    top: f64,
    height: f64,
    noun: &str,
) {
    let max = values.iter().copied().max().unwrap_or(0) as f64;
    let scale = count_gridlines(svg, frame, top, height, max);
    let y = |value: f64| top + height - value / scale * height;

    let points: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, &value)| (frame.x_center(i), y(value as f64)))
        .collect();

    if points.len() > 1 {
        let path: Vec<String> = points
            .iter()
            .map(|(px, py)| format!("{px:.2},{py:.2}"))
            .collect();
        let _ = write!(
            svg,
            r#"<polyline fill="none" stroke="{INK2}" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" points="{}"/>"#,
            path.join(" ")
        );
    }

    // Hover targets for every month, larger than the line itself.
    for ((month, value), (px, py)) in months.iter().zip(values).zip(&points) {
        let _ = write!(
            svg,
            r#"<circle cx="{px:.2}" cy="{py:.2}" r="12" fill="transparent"><title>{}: {} {}</title></circle>"#,
            month.short_label(),
            thousands(*value),
            escape(noun)
        );
    }

    // Selective labels: the peak, the low, and the latest month.
    if let Some(last) = values.len().checked_sub(1) {
        let peak = (0..values.len())
            .max_by_key(|&i| (values[i], std::cmp::Reverse(i)))
            .unwrap_or(last);
        let low = (0..values.len())
            .min_by_key(|&i| (values[i], i))
            .unwrap_or(last);

        // A month that is both the low and the latest is labelled once,
        // below its point, as the low.
        let mut labelled: Vec<(usize, bool)> = vec![(peak, true), (low, false), (last, true)];
        labelled.sort();
        labelled.dedup_by_key(|(i, _)| *i);

        for (i, above) in labelled {
            let (px, py) = points[i];
            let ty = if above { py - 10.0 } else { py + 18.0 };
            let _ = write!(
                svg,
                r#"<circle cx="{px:.2}" cy="{py:.2}" r="4" fill="{INK2}" stroke="{SURFACE}" stroke-width="2"/><text x="{px:.2}" y="{ty:.2}" text-anchor="middle" class="value">{}</text>"#,
                thousands(values[i])
            );
        }
    }
}

/// One stacked column's visible segments, bottom to top, for `values` on a
/// scale of `0..=max` drawn `height` tall from `top`: each segment's index in
/// `values`, its top and height, and whether it is the topmost (rounded).
///
/// Segments too thin to show beside the 2px gap are left out of the drawing,
/// and the gap above each segment comes out of its own height, so the column
/// still ends exactly at its total.
pub fn stacked_segments(
    values: &[f64],
    max: f64,
    top: f64,
    height: f64,
) -> Vec<(usize, f64, f64, bool)> {
    let y = |value: f64| top + height - value / max * height;

    let visible: Vec<usize> = (0..values.len())
        .filter(|&k| values[k] / max * height >= GAP + 0.5)
        .collect();

    let topmost = visible.last().copied();
    let mut base = 0.0;
    let mut out = Vec::new();

    for (k, value) in values.iter().enumerate() {
        if visible.contains(&k) {
            let is_top = Some(k) == topmost;
            let mut segment_top = y(base + value);
            let mut segment_height = value / max * height;

            if !is_top {
                segment_top += GAP;
                segment_height -= GAP;
            }

            out.push((k, segment_top, segment_height, is_top));
        }

        base += value;
    }

    out
}

/// Draws one column segment: rounded on top if it is the topmost, square
/// otherwise, carrying `title` for hover.
pub fn segment(
    svg: &mut String,
    x: f64,
    top: f64,
    height: f64,
    is_top: bool,
    color: &str,
    title: &str,
) {
    if is_top {
        let _ = write!(
            svg,
            r#"<path d="{}" fill="{color}"><title>{}</title></path>"#,
            rounded_top(x, top, BAR_W, height, RADIUS),
            escape(title)
        );
    } else {
        let _ = write!(
            svg,
            r#"<rect x="{x:.2}" y="{top:.2}" width="{BAR_W}" height="{height:.2}" fill="{color}"><title>{}</title></rect>"#,
            escape(title)
        );
    }
}

/// A month's label under its column, `y` from the top, and the year under
/// the first month and every January.
pub fn month_label(svg: &mut String, frame: &Frame, i: usize, month: Month, y: f64) {
    let _ = write!(
        svg,
        r#"<text x="{:.2}" y="{:.0}" text-anchor="middle" class="tick">{}</text>"#,
        frame.x_center(i),
        y,
        month.abbreviation()
    );

    if i == 0 || month.month == 1 {
        let _ = write!(
            svg,
            r#"<text x="{:.2}" y="{:.0}" class="year">{}</text>"#,
            frame.x_center(i) - BAR_W / 2.0,
            y + 18.0,
            month.year
        );
    }
}

/// A legend of `(label, color)` entries in fixed columns from `top`.
/// Returns how many rows it took.
pub fn legend(
    svg: &mut String,
    entries: &[(&str, &str)],
    top: f64,
    columns: usize,
    column_width: f64,
    row_height: f64,
) -> usize {
    for (k, (label, color)) in entries.iter().enumerate() {
        let column = (k % columns) as f64;
        let row = (k / columns) as f64;
        let lx = 20.0 + column * column_width;
        let ly = top + row * row_height;
        let _ = write!(
            svg,
            r#"<rect x="{lx:.0}" y="{:.0}" width="10" height="10" rx="2" fill="{color}"/><text x="{:.0}" y="{ly:.0}" class="legend">{}</text>"#,
            ly - 9.0,
            lx + 16.0,
            escape(label)
        );
    }

    entries.len().div_ceil(columns.max(1))
}

/// Footnote lines from `top`, 15px apart.
pub fn foot(svg: &mut String, lines: &[String], top: f64) {
    for (i, line) in lines.iter().enumerate() {
        let _ = write!(
            svg,
            r#"<text x="20" y="{:.0}" class="foot">{}</text>"#,
            top + 15.0 * i as f64,
            escape(line)
        );
    }
}

/// Closes the SVG.
pub fn close(svg: &mut String) {
    svg.push_str("</svg>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_ends_exactly_at_its_total() {
        let values = [224.0, 56.0, 2.0, 6.0, 11.0, 29.0, 10.0, 0.0, 147.0];
        let total: f64 = values.iter().sum();
        let segments = stacked_segments(&values, total, 300.0, 300.0);

        let (_, top, _, is_top) = segments.last().unwrap();
        assert!(is_top);
        assert!((top - 300.0).abs() < 1e-9);

        // The gap sits above a segment, inside its share, so the bottom one
        // still stands on the baseline.
        let (_, first_top, first_h, _) = segments.first().unwrap();
        assert!((first_top + first_h - 600.0).abs() < 1e-9);
    }

    #[test]
    fn slivers_too_thin_to_show_are_left_out() {
        // 2 of 485 is 1.2px of a 300px column: thinner than the gap.
        let values = [224.0, 56.0, 2.0, 6.0, 11.0, 29.0, 10.0, 0.0, 147.0];
        let shown: Vec<usize> = stacked_segments(&values, 485.0, 0.0, 300.0)
            .iter()
            .map(|s| s.0)
            .collect();

        assert!(!shown.contains(&2));
        assert!(!shown.contains(&7));
        assert!(shown.contains(&0));
    }

    #[test]
    fn only_the_topmost_segment_is_rounded() {
        let rounded: Vec<usize> = stacked_segments(&[100.0, 100.0, 0.0, 100.0], 300.0, 0.0, 300.0)
            .into_iter()
            .filter(|s| s.3)
            .map(|s| s.0)
            .collect();
        assert_eq!(rounded, [3]);
    }

    #[test]
    fn an_empty_column_draws_nothing() {
        assert!(stacked_segments(&[0.0, 0.0], 100.0, 0.0, 300.0).is_empty());
    }

    #[test]
    fn gridline_steps_are_round() {
        assert_eq!(nice_step(2111.0), 1000.0);
        assert_eq!(nice_step(619.0), 250.0);
        assert_eq!(nice_step(80.0), 50.0);
        assert_eq!(nice_step(0.0), 1.0);
    }

    #[test]
    fn the_count_scale_stops_near_the_peak() {
        let mut svg = String::new();
        let scale = count_gridlines(&mut svg, &Frame::new(2), 0.0, 100.0, 2111.0);

        assert!(svg.contains(r#"class="tick">2,000<"#));
        assert!(!svg.contains(r#"class="tick">3,000<"#));
        assert!(scale > 2111.0);
    }

    #[test]
    fn a_long_range_widens_the_chart_instead_of_squeezing_columns() {
        assert_eq!(Frame::new(21).width, MIN_WIDTH);
        assert_eq!(
            Frame::new(48).width,
            PLOT_LEFT + PLOT_RIGHT_PAD + 48.0 * MIN_SLOT
        );
    }

    #[test]
    fn the_line_labels_its_peak_low_and_latest_month() {
        let months: Vec<Month> = (1..=4).map(|m| Month::new(2025, m).unwrap()).collect();
        let mut svg = String::new();
        line_panel(
            &mut svg,
            &Frame::new(4),
            &months,
            &[5, 900, 3, 40],
            0.0,
            100.0,
            "things",
        );

        assert!(svg.contains(r#"class="value">900<"#));
        assert!(svg.contains(r#"class="value">3<"#));
        assert!(svg.contains(r#"class="value">40<"#));
        assert!(!svg.contains(r#"class="value">5<"#));
        assert!(svg.contains("<title>Feb 2025: 900 things</title>"));
    }

    #[test]
    fn a_range_starting_mid_year_still_names_its_year() {
        let frame = Frame::new(3);
        let mut svg = String::new();
        for (i, month) in [(2025, 11), (2025, 12), (2026, 1)].into_iter().enumerate() {
            month_label(
                &mut svg,
                &frame,
                i,
                Month::new(month.0, month.1).unwrap(),
                0.0,
            );
        }

        assert!(svg.contains(r#"class="year">2025<"#));
        assert!(svg.contains(r#"class="year">2026<"#));
        assert_eq!(svg.matches(r#"class="year""#).count(), 2);
    }

    #[test]
    fn the_legend_reports_its_rows() {
        let mut svg = String::new();
        let entries = [("A", "#000"), ("B", "#111"), ("C", "#222"), ("D", "#333")];
        assert_eq!(legend(&mut svg, &entries, 0.0, 3, 240.0, 20.0), 2);
        assert!(svg.contains(">D<"));
    }
}
