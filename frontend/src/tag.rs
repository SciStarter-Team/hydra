//! Tags: free words that projects and events are marked with. A tag that
//! begins `_` or `internal_` (as orrery spells them) is staff's own way of
//! marking a project or event: shown to staff alone, never offered to anyone
//! as a suggestion, and kept through any change someone else makes to the
//! tags.

use std::collections::HashSet;

use common::{insensitivestring::InsensitiveString, message::user::normalize_tag};

/// Whether `tag` is staff's own, hidden from everyone else.
pub fn is_hidden(tag: &str) -> bool {
    const INTERNAL: &str = "internal_";
    tag.starts_with('_')
        || tag
            .get(..INTERNAL.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(INTERNAL))
}

/// Whether someone, staff or not (`is_staff`), is shown `tag`.
pub fn shown_to(tag: &str, is_staff: bool) -> bool {
    is_staff || !is_hidden(tag)
}

/// The tags left by someone, staff or not (`is_staff`), who `typed` these
/// over the tags `stored` before. What's typed is set in the one form tags
/// are stored in ([`normalize_tag`]: no `#`, lowercase), as orrery sets it.
/// Staff set every tag they typed; anyone else sets only those that aren't
/// hidden, and the hidden ones already there, which they weren't shown, stay.
pub fn as_set_by<'a>(
    typed: impl IntoIterator<Item = &'a str>,
    stored: impl IntoIterator<Item = &'a str>,
    is_staff: bool,
) -> HashSet<InsensitiveString> {
    let typed = typed
        .into_iter()
        .map(normalize_tag)
        .filter(|tag| !tag.is_empty() && shown_to(tag, is_staff));
    let kept = stored
        .into_iter()
        .filter(|tag| !is_staff && is_hidden(tag))
        .map(str::to_string);
    typed.chain(kept).map(InsensitiveString::from).collect()
}

/// The tags in `typed`, a comma-separated list as the tag field submits it.
pub fn split(typed: &str) -> impl Iterator<Item = &str> {
    typed
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    fn sorted(tags: HashSet<InsensitiveString>) -> Vec<String> {
        let mut tags: Vec<String> = tags.into_iter().map(|tag| tag.to_string()).collect();
        tags.sort();
        tags
    }

    #[test]
    fn underscored_and_internal_tags_are_hidden() {
        check!(is_hidden("_nasa"));
        check!(is_hidden("internal_review"));
        check!(is_hidden("Internal_Review"));
        check!(!is_hidden("internal"));
        check!(!is_hidden("birds_of_prey"));
        check!(!is_hidden("bioblitz"));
    }

    #[test]
    fn staff_set_the_tags_as_typed() {
        let tags = as_set_by(["_nasa", "birds"], ["_old", "fish"], true);
        check!(sorted(tags) == ["_nasa", "birds"]);
    }

    #[test]
    fn tags_are_set_as_they_are_stored() {
        let tags = as_set_by([" #Birds", "FISH#", "#"], [], false);
        check!(sorted(tags) == ["birds", "fish"]);
    }

    #[test]
    fn others_keep_the_hidden_tags_they_were_not_shown() {
        let tags = as_set_by(["birds", " "], ["_nasa", "internal_check", "fish"], false);
        check!(sorted(tags) == ["_nasa", "birds", "internal_check"]);
    }

    #[test]
    fn others_cannot_add_hidden_tags() {
        let tags = as_set_by(["birds", "_mine", "internal_x"], [], false);
        check!(sorted(tags) == ["birds"]);
    }
}
