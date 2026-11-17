//! Which registrants of an event are Citizen Science Champions or SciStarter
//! Ambassadors.
//!
//! Staff upload one or more registrant exports from Zoom and download either
//! of two lists: the registrants who are Ambassadors, and those who have
//! finished Champion training but are not Ambassadors. Registration happens
//! in Zoom, so the exports are the only record of who registered.
//!
//! This file reads the exports, decides who belongs on each list, and writes
//! the list as CSV. It knows nothing of the web framework or the database:
//! the caller looks up the funnel standing of each address in
//! [`Registrants::addresses`] and hands the result in as [`Standings`].

use std::collections::{BTreeMap, HashMap};

/// Which of the two lists to download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum List {
    Ambassadors,
    Champions,
}

impl List {
    /// The list a form's `list` value names.
    pub fn from_value(value: &str) -> Option<List> {
        match value {
            "ambassadors" => Some(List::Ambassadors),
            "champions" => Some(List::Champions),
            _ => None,
        }
    }

    pub fn filename(self) -> &'static str {
        match self {
            List::Ambassadors => "registrants_ambassadors.csv",
            List::Champions => "registrants_champions_not_ambassadors.csv",
        }
    }
}

/// How far along the Ambassador funnel a Champion has got, as far as these
/// lists care.
///
/// The variants are declared from least to furthest along, and the derived
/// ordering ranks them that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reach {
    /// Finished the training, but the route to Ambassador has closed:
    /// rejected, failed screening, or outside the countries the program
    /// covers.
    Closed,
    /// Finished the training and still on the way to Ambassador.
    Open,
    /// An Ambassador.
    Ambassador,
}

/// Where the account holding a registrant's address stands in the
/// Ambassador funnel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    pub reach: Reach,
    /// The funnel stage, as staff know it.
    pub status: String,
    pub handle: String,
    /// The account's own name, used when Zoom has none.
    pub account_name: String,
    /// When the account reached its stage, as YYYY-MM-DD, or empty when that
    /// is not known.
    pub since: String,
}

/// The funnel standing of each registered address that belongs to an
/// account past the start of the funnel.
#[derive(Debug, Default)]
pub struct Standings {
    /// Keyed by the address in lower case.
    by_address: HashMap<String, Standing>,
}

impl Standings {
    /// Records the standing of an account holding this address.
    ///
    /// When one address belongs to more than one account, the account
    /// furthest along decides: an Ambassador under either account is an
    /// Ambassador, and is then not also listed as a Champion.
    pub fn insert(&mut self, address: &str, standing: Standing) {
        let key = address.to_lowercase();

        match self.by_address.get(&key) {
            Some(held) if held.reach >= standing.reach => {}
            _ => {
                self.by_address.insert(key, standing);
            }
        }
    }

    fn get(&self, address: &str) -> Option<&Standing> {
        self.by_address.get(&address.to_lowercase())
    }
}

/// A person who registered, under one address, for one or more sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Registrant {
    /// The address as they gave it to Zoom.
    email: String,
    /// The name they gave Zoom.
    name: String,
    /// The sessions they registered for, each named by the Zoom topic or,
    /// when the export does not say, by the file it came from.
    sessions: Vec<String>,
    found: Found,
}

/// How a registrant's address was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    /// In the Email column of a table of registrants.
    InEmailColumn,
    /// Somewhere in a file with no Email column, so possibly not a
    /// registrant at all.
    ByScanning,
}

/// The meeting's topic, from the rows Zoom puts before the first table: a
/// heading row with a Topic cell, and the values under it on the next row.
fn meeting_topic(rows: &[Vec<String>]) -> Option<String> {
    let preamble = rows
        .iter()
        .take_while(|row| Columns::from_heading(row).is_none());

    let mut topic_column: Option<usize> = None;

    for row in preamble {
        if let Some(column) = topic_column {
            return row.get(column).filter(|value| !value.is_empty()).cloned();
        }
        topic_column = row
            .iter()
            .position(|cell| cell.eq_ignore_ascii_case("topic"));
    }

    None
}

/// The email addresses written anywhere in a cell, as whole words.
fn addresses_in(cell: &str) -> Vec<String> {
    cell.split(|c: char| c.is_whitespace() || ",;:<>()[]\"'".contains(c))
        .map(|word| word.trim_end_matches('.'))
        .filter(|word| word.contains('@'))
        .filter(|word| word.parse::<lettre::Address>().is_ok())
        .map(str::to_owned)
        .collect()
}

/// Everyone in the uploaded exports, one entry per address, in the order
/// they first appear.
#[derive(Debug, Default)]
pub struct Registrants {
    /// Keyed by the address in lower case, the way accounts are matched.
    by_address: BTreeMap<String, usize>,
    people: Vec<Registrant>,
}

/// The column headings Zoom uses for the address, in upper case.
const EMAIL_HEADINGS: &[&str] = &["EMAIL", "EMAIL ADDRESS", "USER EMAIL"];

impl Registrants {
    /// Adds the registrants in one Zoom export.
    ///
    /// Zoom's exports differ: a meeting's registrant list starts at its
    /// heading row, while the registration and attendee reports open with
    /// rows describing the meeting (its topic among them) and may hold more
    /// than one table, each under its own heading row. So any row with an
    /// Email heading starts a table, and the rows after it that hold an
    /// address are registrants.
    ///
    /// A file with no Email heading anywhere is scanned for addresses
    /// instead. Those addresses may not all be registrants (a host's, or one
    /// given in answer to a question), so they are marked for checking.
    ///
    /// A file that cannot be read, or that yields nobody, is refused with a
    /// message for staff that names it.
    pub fn add_export(&mut self, filename: &str, bytes: &[u8]) -> Result<(), String> {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(bytes);

        let mut rows: Vec<Vec<String>> = Vec::new();

        for record in reader.records() {
            let record = record
                .map_err(|err| format!("{filename} could not be read as a CSV file ({err})."))?;
            rows.push(record.iter().map(|cell| cell.trim().to_owned()).collect());
        }

        let topic = meeting_topic(&rows);
        let session = topic.unwrap_or_else(|| filename.to_owned());

        let found = if rows.iter().any(|row| Columns::from_heading(row).is_some()) {
            self.add_tables(&rows, &session)
        } else {
            self.add_scanned(&rows, &session)
        };

        if found == 0 {
            return Err(format!("{filename} lists no registrants."));
        }

        Ok(())
    }

    /// Adds the rows under each Email heading, returning how many.
    fn add_tables(&mut self, rows: &[Vec<String>], session: &str) -> usize {
        let mut columns: Option<Columns> = None;
        let mut found = 0;

        for row in rows {
            if let Some(heading) = Columns::from_heading(row) {
                columns = Some(heading);
                continue;
            }

            let Some(columns) = &columns else {
                continue;
            };

            let email = row
                .get(columns.email)
                .map(String::as_str)
                .unwrap_or_default();

            if !email.contains('@') {
                continue;
            }

            self.add(email, &columns.name(row), session, Found::InEmailColumn);
            found += 1;
        }

        found
    }

    /// Adds every address anywhere in the rows, returning how many.
    fn add_scanned(&mut self, rows: &[Vec<String>], session: &str) -> usize {
        let mut found = 0;

        for address in rows.iter().flatten().flat_map(|cell| addresses_in(cell)) {
            self.add(&address, "", session, Found::ByScanning);
            found += 1;
        }

        found
    }

    fn add(&mut self, email: &str, name: &str, session: &str, found: Found) {
        let key = email.to_lowercase();

        let index = *self.by_address.entry(key).or_insert_with(|| {
            self.people.push(Registrant {
                email: email.to_owned(),
                name: name.to_owned(),
                sessions: Vec::new(),
                found,
            });
            self.people.len() - 1
        });

        let person = &mut self.people[index];

        if person.name.is_empty() {
            person.name = name.to_owned();
        }

        // Listed under an Email heading in any export settles it.
        if found == Found::InEmailColumn {
            person.found = Found::InEmailColumn;
        }

        if !person.sessions.iter().any(|s| s == session) {
            person.sessions.push(session.to_owned());
        }
    }

    /// Every address, in lower case, for looking up the accounts that hold
    /// them.
    pub fn addresses(&self) -> Vec<String> {
        self.by_address.keys().cloned().collect()
    }

    /// The rows of one list, in the order the registrants first appear.
    /// Closed Champions are on the Champions list only when asked for.
    pub fn list(
        &self,
        list: List,
        include_closed: bool,
        standings: &Standings,
    ) -> Vec<RegistrantCsvRow> {
        self.people
            .iter()
            .filter_map(|person| {
                let standing = standings.get(&person.email)?;

                let wanted = match (list, standing.reach) {
                    (List::Ambassadors, Reach::Ambassador) => true,
                    (List::Champions, Reach::Open) => true,
                    (List::Champions, Reach::Closed) => include_closed,
                    _ => false,
                };

                wanted.then(|| RegistrantCsvRow {
                    name: if person.name.is_empty() {
                        standing.account_name.clone()
                    } else {
                        person.name.clone()
                    },
                    email: person.email.clone(),
                    handle: standing.handle.clone(),
                    status: standing.status.clone(),
                    since: standing.since.clone(),
                    sessions: person.sessions.join("; "),
                    note: match person.found {
                        Found::InEmailColumn => String::new(),
                        Found::ByScanning => SCANNED_NOTE.to_owned(),
                    },
                })
            })
            .collect()
    }
}

/// Where a heading row puts the columns this report reads.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Columns {
    email: usize,
    first_name: Option<usize>,
    last_name: Option<usize>,
    /// A single name column, which some exports have in place of the two.
    name: Option<usize>,
}

impl Columns {
    fn from_heading(cells: &[String]) -> Option<Columns> {
        let find = |wanted: &[&str]| {
            cells
                .iter()
                .position(|cell| wanted.contains(&cell.to_uppercase().as_str()))
        };

        Some(Columns {
            email: find(EMAIL_HEADINGS)?,
            first_name: find(&["FIRST NAME"]),
            last_name: find(&["LAST NAME"]),
            name: find(&["NAME", "NAME (ORIGINAL NAME)", "USER NAME (ORIGINAL NAME)"]),
        })
    }

    fn name(&self, cells: &[String]) -> String {
        let cell = |column: Option<usize>| {
            column
                .and_then(|c| cells.get(c))
                .map(String::as_str)
                .unwrap_or_default()
        };

        let joined = [cell(self.first_name), cell(self.last_name)]
            .iter()
            .filter(|part| !part.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ");

        if joined.is_empty() {
            cell(self.name).to_owned()
        } else {
            joined
        }
    }
}

/// One row of a downloaded list.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct RegistrantCsvRow {
    name: String,
    email: String,
    handle: String,
    status: String,
    /// When the person reached their funnel stage.
    since: String,
    sessions: String,
    /// Says when the address came from a file without an Email column.
    note: String,
}

const SCANNED_NOTE: &str =
    "Found in a file with no Email column: check that this person registered";

/// The heading row of [`csv`], one per field of [`RegistrantCsvRow`] in order.
const CSV_HEADINGS: [&str; 7] = [
    "name", "email", "handle", "status", "since", "sessions", "note",
];

/// A list as a CSV file, with a heading row and CRLF line endings.
///
/// The heading row is written even for an empty list, so that a list with
/// nobody on it still downloads as a table rather than an empty file.
pub fn csv(rows: &[RegistrantCsvRow]) -> Result<Vec<u8>, csv::Error> {
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .has_headers(false)
        .from_writer(vec![]);

    writer.write_record(CSV_HEADINGS)?;

    for row in rows {
        writer.serialize(row)?;
    }

    writer
        .into_inner()
        .map_err(|err| csv::Error::from(err.into_error()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    fn read(exports: &[(&str, &str)]) -> Registrants {
        let mut registrants = Registrants::default();
        for (filename, content) in exports {
            registrants
                .add_export(filename, content.as_bytes())
                .expect("export reads");
        }
        registrants
    }

    fn standing(reach: Reach, status: &str, handle: &str) -> Standing {
        Standing {
            reach,
            status: status.to_owned(),
            handle: handle.to_owned(),
            account_name: format!("{handle} Account"),
            since: "2026-03-04".to_owned(),
        }
    }

    fn standings(entries: &[(&str, Standing)]) -> Standings {
        let mut standings = Standings::default();
        for (address, standing) in entries {
            standings.insert(address, standing.clone());
        }
        standings
    }

    const REGISTRANT_LIST: &str = "\
First Name,Last Name,Email,Registration Time,Approval Status
Ada,Lovelace,ada@example.invalid,2026-11-01 10:00,approved
Grace,Hopper,GRACE@example.invalid,2026-11-02 11:00,approved
";

    const REGISTRATION_REPORT: &str = "\
\u{FEFF}Registration Report
Report Generated:,\"Nov 17, 2026 09:00 AM\"
Topic,ID,Start Time
Do NASA Science LIVE! Comets,123 456 789,\"Dec 2, 2026 07:00 PM\"

First Name,Last Name,Email,Registration Time
Grace,Hopper,grace@example.invalid,2026-11-03 12:00
Katherine,Johnson,katherine@example.invalid,2026-11-04 13:00
";

    #[test]
    fn a_registrant_list_starts_at_its_heading_row() {
        let registrants = read(&[("list.csv", REGISTRANT_LIST)]);

        check!(registrants.people.len() == 2);
        check!(registrants.people[0].name == "Ada Lovelace");
        check!(registrants.people[0].email == "ada@example.invalid");
        check!(registrants.people[0].sessions == vec!["list.csv".to_owned()]);
    }

    #[test]
    fn a_report_is_read_past_its_preamble_and_named_by_its_topic() {
        let registrants = read(&[("report.csv", REGISTRATION_REPORT)]);

        check!(registrants.people.len() == 2);
        check!(registrants.people[1].name == "Katherine Johnson");
        check!(registrants.people[1].sessions == vec!["Do NASA Science LIVE! Comets".to_owned()]);
    }

    #[test]
    fn one_person_in_several_exports_is_one_registrant_with_every_session() {
        let registrants = read(&[
            ("list.csv", REGISTRANT_LIST),
            ("report.csv", REGISTRATION_REPORT),
        ]);

        let grace: Vec<_> = registrants
            .people
            .iter()
            .filter(|p| p.email.eq_ignore_ascii_case("grace@example.invalid"))
            .collect();

        check!(grace.len() == 1);
        check!(
            grace[0].sessions
                == vec![
                    "list.csv".to_owned(),
                    "Do NASA Science LIVE! Comets".to_owned()
                ]
        );
        check!(registrants.addresses().len() == 3);
    }

    #[test]
    fn addresses_are_given_in_lower_case() {
        let registrants = read(&[("list.csv", REGISTRANT_LIST)]);

        check!(
            registrants.addresses()
                == vec![
                    "ada@example.invalid".to_owned(),
                    "grace@example.invalid".to_owned()
                ]
        );
    }

    #[test]
    fn a_single_name_column_is_used_when_there_are_not_two() {
        let registrants = read(&[(
            "attendees.csv",
            "Name (Original Name),User Email\nAlan Turing,alan@example.invalid\n",
        )]);

        check!(registrants.people[0].name == "Alan Turing");
    }

    #[test]
    fn rows_without_an_address_are_not_registrants() {
        let registrants = read(&[(
            "list.csv",
            "First Name,Last Name,Email\nNo,Address,\nAda,Lovelace,ada@example.invalid\n",
        )]);

        check!(registrants.people.len() == 1);
    }

    #[test]
    fn a_file_without_an_email_column_is_scanned_for_addresses() {
        let registrants = read(&[(
            "odd.csv",
            "Topic,ID\nDo NASA Science LIVE! Mars,1\n\nWho,Contact\nAda,\"Ada L <ada@example.invalid>\"\nGrace,grace@example.invalid.\n",
        )]);

        let emails: Vec<_> = registrants
            .people
            .iter()
            .map(|p| p.email.as_str())
            .collect();

        check!(emails == vec!["ada@example.invalid", "grace@example.invalid"]);
        check!(
            registrants
                .people
                .iter()
                .all(|p| p.found == Found::ByScanning)
        );
        check!(registrants.people[0].sessions == vec!["Do NASA Science LIVE! Mars".to_owned()]);
    }

    #[test]
    fn scanned_registrants_carry_a_note_to_check_them() {
        let registrants = read(&[("odd.csv", "Contact\nada@example.invalid\n")]);
        let standings = standings(&[(
            "ada@example.invalid",
            standing(Reach::Ambassador, "Ambassador", "ada"),
        )]);

        let rows = registrants.list(List::Ambassadors, false, &standings);

        check!(rows[0].note == SCANNED_NOTE);
    }

    #[test]
    fn being_listed_under_an_email_heading_anywhere_clears_the_note() {
        let registrants = read(&[
            ("odd.csv", "Contact\nGRACE@example.invalid\n"),
            ("report.csv", REGISTRATION_REPORT),
        ]);
        let standings = standings(&[(
            "grace@example.invalid",
            standing(Reach::Ambassador, "Ambassador", "grace"),
        )]);

        let rows = registrants.list(List::Ambassadors, false, &standings);

        check!(rows.len() == 1);
        check!(rows[0].note.is_empty());
    }

    #[test]
    fn tables_under_an_email_heading_are_not_scanned() {
        // The host's address in the preamble is not a registrant.
        let registrants = read(&[(
            "report.csv",
            "Host,host@example.invalid\nFirst Name,Last Name,Email\nAda,Lovelace,ada@example.invalid\n",
        )]);

        check!(registrants.addresses() == vec!["ada@example.invalid".to_owned()]);
    }

    #[test]
    fn only_whole_addresses_are_taken_from_a_cell() {
        check!(addresses_in("mailto:ada@example.invalid") == vec!["ada@example.invalid"]);
        check!(addresses_in("@scistarter on social media").is_empty());
        check!(addresses_in("a@b@c").is_empty());
        check!(
            addresses_in("ada@example.invalid; grace@example.invalid")
                == vec!["ada@example.invalid", "grace@example.invalid"]
        );
    }

    #[test]
    fn a_file_with_no_addresses_at_all_is_refused_by_name() {
        let mut registrants = Registrants::default();
        let problem = registrants
            .add_export("notes.csv", b"Topic,ID\nSomething,1\n")
            .unwrap_err();

        check!(problem.contains("notes.csv"));
    }

    #[test]
    fn a_file_listing_nobody_is_refused() {
        let mut registrants = Registrants::default();
        let problem = registrants
            .add_export("empty.csv", b"First Name,Last Name,Email\n")
            .unwrap_err();

        check!(problem.contains("no registrants"));
    }

    #[test]
    fn the_lists_split_ambassadors_from_champions() {
        let registrants = read(&[("report.csv", REGISTRATION_REPORT)]);
        let standings = standings(&[
            (
                "grace@example.invalid",
                standing(Reach::Ambassador, "Ambassador", "grace"),
            ),
            (
                "katherine@example.invalid",
                standing(Reach::Open, "Application submitted", "kj"),
            ),
        ]);

        let ambassadors = registrants.list(List::Ambassadors, false, &standings);
        let champions = registrants.list(List::Champions, false, &standings);

        check!(ambassadors.len() == 1);
        check!(ambassadors[0].handle == "grace");
        check!(ambassadors[0].status == "Ambassador");
        check!(ambassadors[0].since == "2026-03-04");

        check!(champions.len() == 1);
        check!(champions[0].handle == "kj");
        check!(champions[0].status == "Application submitted");
    }

    #[test]
    fn closed_champions_are_listed_only_when_asked_for() {
        let registrants = read(&[("list.csv", REGISTRANT_LIST)]);
        let standings = standings(&[(
            "ada@example.invalid",
            standing(Reach::Closed, "Rejected", "ada"),
        )]);

        check!(
            registrants
                .list(List::Champions, false, &standings)
                .is_empty()
        );
        check!(registrants.list(List::Champions, true, &standings).len() == 1);
        check!(
            registrants
                .list(List::Ambassadors, true, &standings)
                .is_empty()
        );
    }

    #[test]
    fn registrants_without_a_standing_are_on_neither_list() {
        let registrants = read(&[("list.csv", REGISTRANT_LIST)]);
        let standings = Standings::default();

        check!(
            registrants
                .list(List::Ambassadors, true, &standings)
                .is_empty()
        );
        check!(
            registrants
                .list(List::Champions, true, &standings)
                .is_empty()
        );
    }

    #[test]
    fn the_account_name_stands_in_when_zoom_has_none() {
        let registrants = read(&[("list.csv", "Email\nada@example.invalid\n")]);
        let standings = standings(&[(
            "ada@example.invalid",
            standing(Reach::Ambassador, "Ambassador", "ada"),
        )]);

        let rows = registrants.list(List::Ambassadors, false, &standings);

        check!(rows[0].name == "ada Account");
    }

    #[test]
    fn standings_are_matched_regardless_of_case() {
        let registrants = read(&[("list.csv", REGISTRANT_LIST)]);
        let standings = standings(&[(
            "Grace@Example.Invalid",
            standing(Reach::Ambassador, "Ambassador", "grace"),
        )]);

        let rows = registrants.list(List::Ambassadors, false, &standings);

        check!(rows.len() == 1);
        check!(rows[0].email == "GRACE@example.invalid");
    }

    #[test]
    fn the_account_furthest_along_decides_a_shared_address() {
        let registrants = read(&[("list.csv", REGISTRANT_LIST)]);
        let standings = standings(&[
            (
                "ada@example.invalid",
                standing(Reach::Open, "Application verified", "ada-old"),
            ),
            (
                "ada@example.invalid",
                standing(Reach::Ambassador, "Ambassador", "ada"),
            ),
            (
                "ada@example.invalid",
                standing(Reach::Closed, "Screening failed", "ada-other"),
            ),
        ]);

        let ambassadors = registrants.list(List::Ambassadors, true, &standings);

        check!(ambassadors.len() == 1);
        check!(ambassadors[0].handle == "ada");
        check!(
            registrants
                .list(List::Champions, true, &standings)
                .is_empty()
        );
    }

    #[test]
    fn an_empty_list_still_has_its_heading_row() {
        let bytes = csv(&[]).unwrap();

        check!(
            String::from_utf8(bytes).unwrap() == "name,email,handle,status,since,sessions,note\r\n"
        );
    }

    #[test]
    fn the_headings_are_the_fields_of_a_row() {
        let row = RegistrantCsvRow {
            name: String::new(),
            email: String::new(),
            handle: String::new(),
            status: String::new(),
            since: String::new(),
            sessions: String::new(),
            note: String::new(),
        };

        let mut writer = csv::WriterBuilder::new()
            .has_headers(true)
            .from_writer(vec![]);
        writer.serialize(&row).unwrap();
        let written = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        check!(written.lines().next() == Some(CSV_HEADINGS.join(",").as_str()));
    }

    #[test]
    fn the_csv_has_a_heading_row_and_crlf_endings() {
        let registrants = read(&[("list.csv", REGISTRANT_LIST)]);
        let standings = standings(&[(
            "ada@example.invalid",
            standing(Reach::Ambassador, "Ambassador", "ada"),
        )]);

        let bytes = csv(&registrants.list(List::Ambassadors, false, &standings)).unwrap();

        check!(
            String::from_utf8(bytes).unwrap()
                == "name,email,handle,status,since,sessions,note\r\n\
                    Ada Lovelace,ada@example.invalid,ada,Ambassador,2026-03-04,list.csv,\r\n"
        );
    }
}
