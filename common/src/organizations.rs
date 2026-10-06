//! Collapsing Library Network sign-ups into the organizations behind them.
//!
//! **Shared file.** Byte-identical in orrery
//! (`src/staff/libraries/organizations.rs`) and hydra
//! (`common/src/organizations.rs`), so that both sites count the same
//! libraries and drop the same map pins. Change both copies together.
//!
//! Sign-ups are one row per *person*: several librarians from the same building
//! each fill the form in, and each types the address their own way. Counting
//! organizations, or dropping a pin per library, means deciding when two rows
//! describe the same place.
//!
//! **Neither the name nor the address settles it alone**, and they fail in
//! opposite directions. Two rows can share an address under different names
//! ("Peoria Public Library" and "Peoria Public Library - Main Branch", both at
//! 8463 W Monroe St), and two rows can share a name with addresses typed
//! differently ("5885 S. 116th St." and "5885 South 116th Street"). So rows are
//! joined when they agree on *either* key, and the join is transitive: A and B
//! agreeing on an address while B and C agree on a name puts all three
//! together.
//!
//! Normalization folds the differences that are spelling rather than substance
//! — street suffixes, compass abbreviations, punctuation, suite numbers — and
//! then refuses to use a key that has been normalized down to something too
//! thin to identify anything. A name in a non-Latin script reduces to the empty
//! string, and an address field holding "California" has no house number;
//! either one, used as a key, silently merges every row that shares its
//! emptiness.

use std::collections::HashMap;

/// The fields of one sign-up that identify the organization behind it.
#[derive(Debug, Clone, Copy)]
pub struct Identity<'a> {
    pub name: &'a str,
    pub address: &'a str,
    pub zip: &'a str,
}

/// Words that describe the kind of thoroughfare rather than which one, and so
/// carry no information once the house number and street name are in hand.
/// Dropping them makes "Ave" and "Avenue" agree. "St" is ambiguous between
/// Street and Saint; dropping it either way is consistent across rows, which
/// is all that matters here.
const STREET_SUFFIXES: &[&str] = &[
    "street",
    "st",
    "avenue",
    "ave",
    "road",
    "rd",
    "drive",
    "dr",
    "boulevard",
    "blvd",
    "lane",
    "ln",
    "court",
    "ct",
    "place",
    "pl",
    "parkway",
    "pkwy",
    "highway",
    "hwy",
    "circle",
    "cir",
    "terrace",
    "way",
    "square",
    "sq",
];

/// Compass words folded to their abbreviation. Folded rather than dropped: a
/// row writing "S" and another writing "South" must end up the same, and
/// dropping only the spelled-out form would leave them different.
///
/// Longest first, so "northeast" is not consumed by the "north" rule.
const DIRECTIONS: &[(&str, &str)] = &[
    ("northeast", "ne"),
    ("northwest", "nw"),
    ("southeast", "se"),
    ("southwest", "sw"),
    ("north", "n"),
    ("south", "s"),
    ("east", "e"),
    ("west", "w"),
];

/// Tokens introducing a sub-address, which identifies a room rather than a
/// building. The token and the designator following it are both dropped.
const SUBUNIT_MARKERS: &[&str] = &["suite", "ste", "unit", "apt", "room", "rm", "floor", "fl"];

/// Shortest name that may be used as a join key. Below this a name is initials
/// or punctuation, too thin to be evidence that two rows are the same place.
const MIN_NAME_KEY: usize = 4;

/// Shortest normalized address that may be used as a join key.
const MIN_ADDRESS_KEY: usize = 5;

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// The organization name reduced to its letters and digits.
///
/// Returns `None` when nothing usable survives — most often a name written in
/// a script with no ASCII letters at all.
pub fn normalize_name(name: &str) -> Option<String> {
    let key: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();

    (key.len() >= MIN_NAME_KEY).then_some(key)
}

/// A street address reduced to house number, street name and compass point,
/// paired with its postal code.
///
/// Returns `None` when the result could not identify a building: a PO box, a
/// field holding a city or a country rather than an address, or anything with
/// no house number in it.
pub fn normalize_address(address: &str, zip: &str) -> Option<String> {
    // Only the first line, and only up to the first comma: people paste the
    // whole mailing address into this field, and everything after the street
    // repeats the city, state and zip we already hold separately.
    let head = address
        .split(['\n', '\r'])
        .next()
        .unwrap_or("")
        .split(',')
        .next()
        .unwrap_or("");

    // A "#" always introduces a unit or suite designator, and the digits
    // after it would otherwise read as part of the street address: real rows
    // hold both "157 Main Street" and "157 Main Street (#789)".
    let head = head.split('#').next().unwrap_or("");

    let tokens = tokenize(head);

    // A post office box names no building, and its number would happily
    // collide with a house number somewhere else in the same zip.
    if tokens.iter().any(|t| t == "box") {
        return None;
    }

    let mut key = String::new();
    let mut skip_next = false;

    for token in &tokens {
        if skip_next {
            skip_next = false;
            continue;
        }

        if SUBUNIT_MARKERS.contains(&token.as_str()) {
            skip_next = true;
            continue;
        }

        if STREET_SUFFIXES.contains(&token.as_str()) {
            continue;
        }

        match DIRECTIONS.iter().find(|(word, _)| word == token) {
            Some((_, abbreviation)) => key.push_str(abbreviation),
            None => key.push_str(token),
        }
    }

    // Every real street address carries a number. Without one this is a place
    // name — "California", "Tehran" — which describes a region, not a door.
    if key.len() < MIN_ADDRESS_KEY || !key.contains(|c: char| c.is_ascii_digit()) {
        return None;
    }

    Some(format!("{key}|{}", zip.trim()))
}

/// Union-find over the sign-up rows, so that agreement on either key is
/// transitive.
struct DisjointSet {
    parent: Vec<usize>,
}

impl DisjointSet {
    fn new(size: usize) -> Self {
        DisjointSet {
            parent: (0..size).collect(),
        }
    }

    fn find(&mut self, mut index: usize) -> usize {
        while self.parent[index] != index {
            // Path halving: point at the grandparent on the way up, which
            // keeps the trees flat without a second pass.
            self.parent[index] = self.parent[self.parent[index]];
            index = self.parent[index];
        }
        index
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            // Always point at the lower index, so a group's representative is
            // its earliest row and the result does not depend on input order
            // beyond that.
            let (keep, merge) = if a < b { (a, b) } else { (b, a) };
            self.parent[merge] = keep;
        }
    }
}

/// Assign every sign-up to an organization.
///
/// Returns one group identifier per input row, in the same order; rows sharing
/// an identifier are the same organization. The identifier is the index of the
/// earliest row in the group, so callers that pass rows in date order get a
/// stable, meaningful representative.
pub fn group(rows: &[Identity<'_>]) -> Vec<usize> {
    let mut set = DisjointSet::new(rows.len());
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut by_address: HashMap<String, usize> = HashMap::new();

    for (index, row) in rows.iter().enumerate() {
        if let Some(key) = normalize_name(row.name) {
            match by_name.entry(key) {
                std::collections::hash_map::Entry::Occupied(seen) => set.union(index, *seen.get()),
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(index);
                }
            }
        }

        if let Some(key) = normalize_address(row.address, row.zip) {
            match by_address.entry(key) {
                std::collections::hash_map::Entry::Occupied(seen) => set.union(index, *seen.get()),
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(index);
                }
            }
        }
    }

    (0..rows.len()).map(|index| set.find(index)).collect()
}

/// The indexes of the first row of each organization, in input order.
///
/// Which row stands for an organization is the caller's choice of ordering:
/// oldest first gives the moment it joined the network, newest first its
/// freshest description.
pub fn first_of_each(rows: &[Identity<'_>]) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();

    group(rows)
        .into_iter()
        .enumerate()
        .filter(|(_, group)| seen.insert(*group))
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity<'a>(name: &'a str, address: &'a str, zip: &'a str) -> Identity<'a> {
        Identity { name, address, zip }
    }

    /// Count the organizations in a set of rows, the way the ledger does.
    fn organizations(rows: &[Identity<'_>]) -> usize {
        let groups = group(rows);
        let unique: std::collections::HashSet<usize> = groups.into_iter().collect();
        unique.len()
    }

    #[test]
    fn the_first_row_of_each_organization_stands_for_it() {
        let rows = [
            identity("Hales Corners Library", "5885 S. 116th St.", "53130"),
            identity("Frisco Public Library", "8000 Dallas Pkwy", "75034"),
            identity("Hales Corners Library", "5885 South 116th Street", "53130"),
            identity("Frisco Public Library", "8000 Dallas Parkway", "75034"),
            identity("Wiggin Memorial Library", "10 Bunker Hill Ave", "03885"),
        ];

        assert_eq!(first_of_each(&rows), [0, 1, 4]);
        assert!(first_of_each(&[]).is_empty());
    }

    #[test]
    fn one_building_under_two_names_is_one_organization() {
        // Both of these are in the roster at the same door.
        let rows = [
            identity("Peoria Public Library", "8463 W Monroe St", "85345"),
            identity(
                "Peoria Public Library - Main Branch",
                "8463 W. Monroe St.",
                "85345",
            ),
        ];

        assert_eq!(organizations(&rows), 1);
    }

    #[test]
    fn one_name_with_the_address_typed_differently_is_one_organization() {
        let rows = [
            identity("Hales Corners Library", "5885 S. 116th St.", "53130"),
            identity("Hales Corners Library", "5885 South 116th Street", "53130"),
        ];

        assert_eq!(organizations(&rows), 1);
    }

    #[test]
    fn agreement_on_either_key_is_transitive() {
        // A and B share an address; B and C share a name. All three are one
        // organization even though A and C agree on neither.
        let rows = [
            identity("Starkville Public Library", "326 University Drive", "39759"),
            identity(
                "Starkville-Oktibbeha County Public Library System",
                "326 University Dr.",
                "39759",
            ),
            identity(
                "Starkville-Oktibbeha County Public Library System",
                "1 Somewhere Else",
                "39759",
            ),
        ];

        let groups = group(&rows);
        assert_eq!(groups[0], groups[1]);
        assert_eq!(groups[1], groups[2]);
    }

    #[test]
    fn a_pasted_mailing_address_still_matches_the_street() {
        let rows = [
            identity("Arlington County Public Library", "1015 N Quincy", "22201"),
            identity(
                "Arlington Public Library (VA)",
                "1015 N. Quincy St\r\nArlington VA 22206",
                "22201",
            ),
        ];

        assert_eq!(organizations(&rows), 1);
    }

    #[test]
    fn different_branches_stay_separate() {
        let rows = [
            identity(
                "Avondale Public Library",
                "11350 W Civic Center Dr",
                "85323",
            ),
            identity(
                "Avondale Public Library-Sam Garcia Branch",
                "495 E Western Ave",
                "85323",
            ),
        ];

        assert_eq!(organizations(&rows), 2);
    }

    #[test]
    fn a_name_in_another_script_does_not_merge_with_everything() {
        // Neither name yields an ASCII key. If the empty key were allowed to
        // join, these two unrelated organizations would become one.
        let rows = [
            identity("アースレンジャー地球環境アカデミー", "日本 広島市", ""),
            identity("地球环境学院", "北京", ""),
        ];

        assert_eq!(organizations(&rows), 2);
    }

    #[test]
    fn a_region_in_the_address_field_does_not_merge_with_everything() {
        let rows = [
            identity("NASA and AIAA", "California", "94035"),
            identity("University of Dr. Morteza Chamanara", "California", "94035"),
        ];

        assert_eq!(organizations(&rows), 2);
        assert_eq!(normalize_address("California", "94035"), None);
    }

    #[test]
    fn a_post_office_box_is_not_a_building() {
        assert_eq!(normalize_address("P.O. Box 283", "07976"), None);

        // ...but a street address that merely mentions one still matches the
        // street, because everything after the comma is dropped.
        let rows = [
            identity(
                "Kemmerer Library",
                "19 Blue Mill Road, P.O. Box 283",
                "07976",
            ),
            identity(
                "Kemmerer Library Harding Township",
                "19 Blue Mill Road",
                "07976",
            ),
        ];

        assert_eq!(organizations(&rows), 1);
    }

    #[test]
    fn a_unit_number_written_with_a_hash_does_not_split_a_building() {
        let rows = [
            identity(
                "J. Robert Jamerson Memorial Library",
                "157 Main Street",
                "24522",
            ),
            identity("Jamerson Memorial", "157 Main Street (#789)", "24522"),
        ];

        assert_eq!(organizations(&rows), 1);
    }

    #[test]
    fn a_suite_number_does_not_split_a_building() {
        let rows = [
            identity("Some Institute", "3033 Wilson Blvd Suite 700", "22201"),
            identity("Some Institute Annex", "3033 Wilson Blvd", "22201"),
        ];

        assert_eq!(organizations(&rows), 1);
    }

    #[test]
    fn the_same_street_number_in_different_zips_is_not_the_same_place() {
        let rows = [
            identity("First Library", "100 Main Street", "10001"),
            identity("Second Library", "100 Main Street", "90210"),
        ];

        assert_eq!(organizations(&rows), 2);
    }

    #[test]
    fn a_group_is_named_by_its_earliest_row() {
        let rows = [
            identity("Alpha Library", "1 First Street", "10001"),
            identity("Beta Library", "2 Second Street", "10002"),
            identity("Alpha Library", "1 First St", "10001"),
        ];

        let groups = group(&rows);
        assert_eq!(groups, vec![0, 1, 0]);
    }

    #[test]
    fn compass_words_fold_to_their_abbreviation() {
        assert_eq!(
            normalize_address("100 Northeast Main Street", "10001"),
            normalize_address("100 NE Main St", "10001")
        );
        // ...and are not simply dropped, which would make these two agree.
        assert_ne!(
            normalize_address("100 North Main Street", "10001"),
            normalize_address("100 South Main Street", "10001")
        );
    }

    #[test]
    fn an_empty_input_has_no_organizations() {
        assert_eq!(organizations(&[]), 0);
    }
}
