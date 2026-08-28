//! GET/POST `/staff/compare-emails` — line up two CSV email lists against each
//! other. Ported from orrery's `compare_emails`.
//!
//! Staff use this to reconcile a list someone sent them against a list the site
//! produced. Addresses that appear in both are reported first; the rest are
//! paired off by edit distance, so a typo or a changed domain still lands next
//! to the address it probably means rather than being reported twice as
//! missing.

use poem::{Response, handler, web::Multipart};

use super::csv_field;
use crate::component::html_escape;

/// Levenshtein edit distance, two rows of the usual matrix.
///
/// The comparison is over `char`s rather than bytes so a non-ASCII local part
/// costs one edit per character, not one per UTF-8 byte.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();

    if a.is_empty() {
        return b.len();
    }

    let mut prev: Vec<usize> = (0..=a.len()).collect();
    let mut cur = vec![0usize; a.len() + 1];

    for (j, bc) in b.iter().enumerate() {
        cur[0] = j + 1;
        for (i, ac) in a.iter().enumerate() {
            let substitution = prev[i] + usize::from(ac != bc);
            let deletion = prev[i + 1] + 1;
            let insertion = cur[i] + 1;
            cur[i + 1] = substitution.min(deletion).min(insertion);
        }
        std::mem::swap(&mut prev, &mut cur);
    }

    prev[a.len()]
}

/// One uploaded list: the addresses it holds, keyed by their normalized form.
///
/// Keyed rather than a plain list because matching is done on the normalized
/// form while the report shows what was actually in the file. A repeated
/// address collapses to one entry, as it does in orrery.
struct Column {
    /// Normalized address -> the address as it appeared in the file.
    values: std::collections::BTreeMap<String, String>,
}

impl Column {
    /// Read `column` (or the first column, if unnamed) out of a CSV.
    fn parse(data: &[u8], column: &str, ignore_domain: bool) -> Result<Column, anyhow::Error> {
        let mut reader = csv::Reader::from_reader(data);

        let headers = reader.headers()?.clone();
        let index = if column.is_empty() {
            0
        } else {
            headers
                .iter()
                .position(|h| h.trim().eq_ignore_ascii_case(column.trim()))
                .ok_or_else(|| anyhow::anyhow!("No column named {column:?} in the uploaded file"))?
        };

        let mut values = std::collections::BTreeMap::new();

        for record in reader.records() {
            let record = record?;
            let raw = record.get(index).unwrap_or_default().trim();

            if raw.is_empty() {
                continue;
            }

            let lowered = raw.to_lowercase();
            let key = if ignore_domain {
                lowered
                    .split_once('@')
                    .map(|(local, _)| local.to_string())
                    .unwrap_or(lowered)
            } else {
                lowered
            };

            values.insert(key, raw.to_string());
        }

        Ok(Column { values })
    }
}

/// A row of the report: what was found in each file, and how far apart they are.
///
/// [`Pairing::UNMATCHED`] is the distance recorded for an address that found no
/// counterpart at all. orrery uses the same sentinel, which sorts those rows
/// last without needing a separate pass.
struct Pairing {
    a: String,
    b: String,
    distance: usize,
}

impl Pairing {
    const UNMATCHED: usize = 99;
}

/// Pair the two lists up: exact matches first, then each remaining address in
/// `a` with its closest survivor in `b`.
///
/// The pairing is greedy in `a`'s order, and a match is only accepted if it is
/// closer than rewriting the address from scratch — the same rule orrery
/// applies, which keeps two unrelated addresses from being reported as a near
/// miss for each other.
fn compare(a: Column, b: Column) -> Vec<Pairing> {
    let mut a = a.values;
    let mut b = b.values;
    let mut results = Vec::new();

    let exact: Vec<String> = a.keys().filter(|k| b.contains_key(*k)).cloned().collect();

    for key in exact {
        let a_val = a.remove(&key).unwrap_or_default();
        let b_val = b.remove(&key).unwrap_or_default();
        results.push(Pairing {
            a: a_val,
            b: b_val,
            distance: 0,
        });
    }

    let remaining: Vec<String> = a.keys().cloned().collect();

    for key in remaining {
        let mut best: Option<(String, usize)> = None;

        for candidate in b.keys() {
            let distance = levenshtein(&key, candidate);
            if distance
                < best
                    .as_ref()
                    .map(|(_, d)| *d)
                    .unwrap_or(key.chars().count())
            {
                best = Some((candidate.clone(), distance));
            }
        }

        let a_val = a.remove(&key).unwrap_or_default();

        match best {
            Some((candidate, distance)) => {
                let b_val = b.remove(&candidate).unwrap_or_default();
                results.push(Pairing {
                    a: a_val,
                    b: b_val,
                    distance,
                });
            }
            None => results.push(Pairing {
                a: a_val,
                b: String::new(),
                distance: Pairing::UNMATCHED,
            }),
        }
    }

    for b_val in b.into_values() {
        results.push(Pairing {
            a: String::new(),
            b: b_val,
            distance: Pairing::UNMATCHED,
        });
    }

    results.sort_by_key(|p| p.distance);
    results
}

const PAGE_STYLE: &str = "<style>body{font-family:sans-serif;margin:2rem;max-width:900px;}\
     table{border-collapse:collapse;width:100%;margin-top:1rem;}\
     td,th{padding:4px 8px;text-align:left;border-bottom:1px solid #eee;}\
     th{border-bottom:1px solid #ccc;}\
     .num{text-align:right;font-variant-numeric:tabular-nums;}\
     .exact{color:#0a7d33;}.far{color:#999;}\
     fieldset{border:1px solid #ddd;margin-bottom:1rem;}\
     label{display:block;margin:.35rem 0;}</style>";

fn form_html(error: Option<&str>) -> String {
    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>Compare Emails</title>",
    );
    html.push_str(PAGE_STYLE);
    html.push_str("</head><body><h1>Compare email lists</h1>");

    if let Some(error) = error {
        html.push_str(&format!(
            "<p style=\"color:#b00\">{}</p>",
            html_escape(error)
        ));
    }

    html.push_str(
        "<form method=\"post\" enctype=\"multipart/form-data\">\
         <fieldset><legend>First list</legend>\
         <label>CSV file <input type=\"file\" name=\"csv_a\" accept=\".csv,text/csv\" required></label>\
         <label>Column name <input type=\"text\" name=\"csv_a_column\" placeholder=\"(first column)\"></label>\
         </fieldset>\
         <fieldset><legend>Second list</legend>\
         <label>CSV file <input type=\"file\" name=\"csv_b\" accept=\".csv,text/csv\" required></label>\
         <label>Column name <input type=\"text\" name=\"csv_b_column\" placeholder=\"(first column)\"></label>\
         </fieldset>\
         <label><input type=\"checkbox\" name=\"ignore_domain\" value=\"1\"> \
         Compare only the part before the @</label>\
         <p><button type=\"submit\">Compare</button></p></form>",
    );

    html.push_str("</body></html>");
    html
}

/// The uploaded form, as far as it was filled in.
#[derive(Default)]
struct Upload {
    csv_a: Option<Vec<u8>>,
    csv_b: Option<Vec<u8>>,
    csv_a_column: String,
    csv_b_column: String,
    ignore_domain: bool,
}

impl Upload {
    async fn read(mut multipart: Multipart) -> Result<Upload, anyhow::Error> {
        use tokio::io::AsyncReadExt;

        let mut upload = Upload::default();

        while let Some(field) = multipart.next_field().await? {
            let name = field.name().unwrap_or_default().to_string();

            match name.as_str() {
                "csv_a" | "csv_b" => {
                    let mut data = Vec::new();
                    field.into_async_read().read_to_end(&mut data).await?;

                    if data.is_empty() {
                        continue;
                    }

                    if name == "csv_a" {
                        upload.csv_a = Some(data);
                    } else {
                        upload.csv_b = Some(data);
                    }
                }
                "csv_a_column" => upload.csv_a_column = field.text().await?,
                "csv_b_column" => upload.csv_b_column = field.text().await?,
                "ignore_domain" => upload.ignore_domain = !field.text().await?.is_empty(),
                _ => {}
            }
        }

        Ok(upload)
    }
}

/// GET `/staff/compare-emails` — the upload form.
#[handler]
pub async fn form() -> Response {
    Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(form_html(None))
}

/// POST `/staff/compare-emails` — the comparison of the two uploaded lists.
///
/// A missing file or an unknown column name is reported back on the form
/// rather than as an error page, because both are ordinary mistakes to make
/// with a spreadsheet someone else exported.
#[handler]
#[tracing::instrument(skip(multipart))]
pub async fn compare_emails(multipart: Multipart) -> Result<Response, anyhow::Error> {
    let upload = Upload::read(multipart).await?;

    let (Some(csv_a), Some(csv_b)) = (upload.csv_a, upload.csv_b) else {
        return Ok(Response::builder()
            .content_type("text/html; charset=utf-8")
            .body(form_html(Some(
                "Both files are required, and neither may be empty.",
            ))));
    };

    let columns = Column::parse(&csv_a, &upload.csv_a_column, upload.ignore_domain).and_then(|a| {
        Column::parse(&csv_b, &upload.csv_b_column, upload.ignore_domain).map(|b| (a, b))
    });

    let (a, b) = match columns {
        Ok(columns) => columns,
        Err(error) => {
            return Ok(Response::builder()
                .content_type("text/html; charset=utf-8")
                .body(form_html(Some(&error.to_string()))));
        }
    };

    let results = compare(a, b);

    let exact = results.iter().filter(|p| p.distance == 0).count();
    let unmatched = results
        .iter()
        .filter(|p| p.distance == Pairing::UNMATCHED)
        .count();

    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>Compare Emails</title>",
    );
    html.push_str(PAGE_STYLE);
    html.push_str(&format!(
        "</head><body><h1>Comparison</h1>\
         <p>{} rows — {exact} exact, {unmatched} with no counterpart.</p>\
         <p><a href=\"/staff/compare-emails\">&laquo; compare another pair</a></p>\
         <table><tr><th>First list</th><th>Second list</th><th class=\"num\">Distance</th></tr>",
        results.len(),
    ));

    for pairing in &results {
        let class = match pairing.distance {
            0 => "exact",
            Pairing::UNMATCHED => "far",
            _ => "",
        };
        let distance = if pairing.distance == Pairing::UNMATCHED {
            String::from("—")
        } else {
            pairing.distance.to_string()
        };
        html.push_str(&format!(
            "<tr class=\"{class}\"><td>{}</td><td>{}</td><td class=\"num\">{distance}</td></tr>",
            html_escape(&pairing.a),
            html_escape(&pairing.b),
        ));
    }

    html.push_str("</table><h2>As CSV</h2><pre>first list,second list,distance\n");
    for pairing in &results {
        html.push_str(&html_escape(&format!(
            "{},{},{}\n",
            csv_field(&pairing.a),
            csv_field(&pairing.b),
            pairing.distance,
        )));
    }
    html.push_str("</pre></body></html>");

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_counts_single_edits() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
        assert_eq!(levenshtein("abc", "ab"), 1);
        assert_eq!(levenshtein("ab", "abc"), 1);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn levenshtein_counts_characters_not_bytes() {
        // "é" is two bytes; changing it is one edit, not two.
        assert_eq!(levenshtein("café", "cafe"), 1);
    }

    fn column(rows: &[&str], ignore_domain: bool) -> Column {
        let csv = format!("email\n{}\n", rows.join("\n"));
        Column::parse(csv.as_bytes(), "", ignore_domain).expect("parses")
    }

    #[test]
    fn exact_matches_are_reported_first_and_keep_original_spelling() {
        let results = compare(
            column(&["Alice@Example.com"], false),
            column(&["alice@example.COM"], false),
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].distance, 0);
        assert_eq!(results[0].a, "Alice@Example.com");
        assert_eq!(results[0].b, "alice@example.COM");
    }

    #[test]
    fn near_misses_are_paired_by_edit_distance() {
        let results = compare(
            column(&["alice@example.com"], false),
            column(&["alcie@example.com"], false),
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].distance, 2);
    }

    #[test]
    fn an_address_with_no_counterpart_is_reported_alone() {
        let results = compare(column(&["alice@example.com"], false), column(&[], false));

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].a, "alice@example.com");
        assert_eq!(results[0].b, "");
        assert_eq!(results[0].distance, Pairing::UNMATCHED);
    }

    #[test]
    fn addresses_only_in_the_second_list_are_reported_too() {
        let results = compare(column(&[], false), column(&["bob@example.com"], false));

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].a, "");
        assert_eq!(results[0].b, "bob@example.com");
    }

    #[test]
    fn ignoring_the_domain_makes_different_domains_an_exact_match() {
        let results = compare(
            column(&["alice@example.com"], true),
            column(&["alice@other.org"], true),
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].distance, 0);
    }

    #[test]
    fn a_named_column_is_read_instead_of_the_first() {
        let csv = b"name,email\nAlice,alice@example.com\n";
        let column = Column::parse(csv, "email", false).expect("parses");

        assert_eq!(
            column.values.get("alice@example.com").map(String::as_str),
            Some("alice@example.com")
        );
    }

    #[test]
    fn an_unknown_column_name_is_an_error() {
        let csv = b"name,email\nAlice,alice@example.com\n";
        assert!(Column::parse(csv, "address", false).is_err());
    }

    #[test]
    fn every_address_appears_exactly_once() {
        // Two lists that share nothing: each address is reported, and the
        // greedy pairing never consumes one twice.
        let results = compare(
            column(&["a@x.com", "b@x.com"], false),
            column(&["c@y.com"], false),
        );

        let seen_a: Vec<&str> = results
            .iter()
            .map(|p| p.a.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        let seen_b: Vec<&str> = results
            .iter()
            .map(|p| p.b.as_str())
            .filter(|s| !s.is_empty())
            .collect();

        assert_eq!(seen_a.len(), 2);
        assert_eq!(seen_b.len(), 1);
    }
}
