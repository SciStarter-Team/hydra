//! orrery's ids for the choice values hydra keeps by name.
//!
//! orrery stores a choice field as ids into `core_choicevalue`, and its v1
//! project API accepts a value as either that id or the value's exact name
//! (`Choice.choice_value`). Partners written against orrery may send either,
//! so the API keeps accepting both. The names are the vocabularies in
//! [`super::field`]; the tests keep the two from drifting apart.

use serde_json::Value;

pub const AUDIENCE: &[(i64, &str)] = &[
    (111, "Elementary school (6 - 10 years)"),
    (112, "Middle school (11 - 13 years)"),
    (113, "High school (14 - 17 years)"),
    (114, "College"),
    (115, "Graduate students"),
    (116, "Adults"),
    (117, "Families"),
    (118, "Seniors"),
];

pub const TOPICS: &[(i64, &str)] = &[
    (1, "Social Science"),
    (2, "Geography"),
    (3, "Agriculture"),
    (4, "Events"),
    (5, "Psychology"),
    (6, "Transportation"),
    (7, "Crowd Funding"),
    (8, "Physics"),
    (9, "Ecology & Environment"),
    (10, "Chemistry"),
    (11, "Birds"),
    (12, "Biology"),
    (13, "Astronomy & Space"),
    (14, "Ocean, Water, Marine & Terrestrial"),
    (15, "History, Archeology & Culture"),
    (16, "Insects & Pollinators"),
    (17, "Sound"),
    (18, "Computers & Technology"),
    (19, "Geology & Earth Science"),
    (20, "Animals"),
    (21, "Nature & Outdoors"),
    (22, "Awards"),
    (23, "Health & Medicine"),
    (24, "Education"),
    (25, "Food"),
    (26, "Science Policy"),
    (27, "Climate & Weather"),
    (203, "Disaster Response"),
];

pub const ACTIVITIES: &[(i64, &str)] = &[
    (28, "On a lunch break"),
    (29, "At sports stadiums"),
    (30, "At night"),
    (31, "At a science center, zoo or aquarium"),
    (32, "At school"),
    (33, "On a walk, run"),
    (34, "Exclusively online"),
    (35, "At the beach"),
    (36, "In snow or rain"),
    (37, "At home"),
    (38, "In the car"),
    (39, "Emergency Response"),
    (40, "On a hike"),
    (41, "While fishing"),
    (42, "In oceans, streams, rivers, lakes"),
];

/// The choice value a partner names, by its orrery id (a JSON integer) or by
/// its exact name, or None if it names none of `choices`.
pub fn named(choices: &[(i64, &'static str)], value: &Value) -> Option<&'static str> {
    choices
        .iter()
        .find(|(id, name)| match value {
            Value::Number(number) => number.as_i64() == Some(*id),
            Value::String(text) => text == name,
            _ => false,
        })
        .map(|(_, name)| *name)
}

/// orrery's id for the choice value `name`, or None if `choices` has no
/// value by that name.
pub fn id_of(choices: &[(i64, &'static str)], name: &str) -> Option<i64> {
    choices
        .iter()
        .find(|(_, held)| *held == name)
        .map(|(id, _)| *id)
}

/// The choice values a partner's field names, in order and without
/// repeats. A list names one value per item; anything else names one value.
/// Values that name nothing are dropped, as orrery drops them.
pub fn all_named(choices: &[(i64, &'static str)], value: &Value) -> Box<[String]> {
    let items = match value {
        Value::Array(items) => items.as_slice(),
        single => std::slice::from_ref(single),
    };

    let mut names: Vec<String> = Vec::new();

    for name in items.iter().filter_map(|item| named(choices, item)) {
        if !names.iter().any(|held| held == name) {
            names.push(name.to_string());
        }
    }

    names.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::field::{ACTIVITIES_VOCABULARY, AUDIENCE_VOCABULARY, TOPICS_VOCABULARY};
    use assert2::check;
    use serde_json::json;

    fn names(choices: &[(i64, &str)]) -> Vec<String> {
        let mut names: Vec<String> = choices.iter().map(|(_, name)| name.to_string()).collect();
        names.sort();
        names
    }

    fn vocabulary(words: &[&str]) -> Vec<String> {
        let mut words: Vec<String> = words.iter().map(|word| word.to_string()).collect();
        words.sort();
        words
    }

    #[test]
    fn the_ids_cover_the_vocabularies_exactly() {
        check!(names(AUDIENCE) == vocabulary(AUDIENCE_VOCABULARY));
        check!(names(TOPICS) == vocabulary(TOPICS_VOCABULARY));
        check!(names(ACTIVITIES) == vocabulary(ACTIVITIES_VOCABULARY));
    }

    #[test]
    fn a_value_is_named_by_id_or_exact_name() {
        check!(named(AUDIENCE, &json!(116)) == Some("Adults"));
        check!(named(AUDIENCE, &json!("Adults")) == Some("Adults"));
        check!(named(AUDIENCE, &json!("adults")) == None);
        check!(named(AUDIENCE, &json!("116")) == None);
        check!(named(AUDIENCE, &json!(1)) == None);
    }

    #[test]
    fn a_string_is_one_value_even_with_a_comma_in_it() {
        check!(
            all_named(TOPICS, &json!("History, Archeology & Culture")).to_vec()
                == vec!["History, Archeology & Culture".to_string()]
        );
    }

    #[test]
    fn a_list_drops_what_names_nothing() {
        check!(
            all_named(AUDIENCE, &json!([116, "Families", "Pirates", 116])).to_vec()
                == vec!["Adults".to_string(), "Families".to_string()]
        );
    }
}
