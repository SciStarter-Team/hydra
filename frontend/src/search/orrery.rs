//! Links written for orrery's finder, translated into hydra's.
//!
//! Pages, emails and other sites link into the finder with the query string
//! orrery's finder reads (`frontend/static/components/project_finder` there):
//! `phrase`; `location` with `latitude` and `longitude`; `topic`, `activity`
//! and `audience` as orrery's choice ids; `filters` as numbered checkboxes; and
//! the `events`, `projects` and `active` switches. The finder redirects such a
//! link to the hydra search that asks the same thing, so that the link keeps
//! working and the page it lands on describes its search in the sidebar's own
//! terms.
//!
//! Everything else in the query string rides along as it came: `utm_*` and
//! the like, hydra's own parameters, and the parameters orrery read that hydra
//! has no counterpart for (`tag`, `SDG`, `back`), which the finder ignores.

use url::form_urlencoded;

use super::{FinderQuery, MiscFilter, SearchKinds};
use crate::geocode::locate;
use crate::project::choice_ids;

/// A parameter orrery's finder reads, which a hydra link never carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OrreryParam {
    Phrase,
    Location,
    Latitude,
    Longitude,
    Activity,
    Topic,
    Audience,
    Filters,
    Events,
    Projects,
    Active,
}

impl OrreryParam {
    fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "phrase" => OrreryParam::Phrase,
            "location" => OrreryParam::Location,
            "latitude" => OrreryParam::Latitude,
            "longitude" => OrreryParam::Longitude,
            "activity" => OrreryParam::Activity,
            "topic" => OrreryParam::Topic,
            "audience" => OrreryParam::Audience,
            "filters" => OrreryParam::Filters,
            "events" => OrreryParam::Events,
            "projects" => OrreryParam::Projects,
            "active" => OrreryParam::Active,
            _ => return None,
        })
    }
}

/// What one of orrery's numbered `filters` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OrreryFilter {
    Misc(MiscFilter),

    /// Entirely online, which hydra calls remote.
    Online,

    /// Every status, finished projects included.
    AnyStatus,
}

impl OrreryFilter {
    /// The filter orrery numbers `id`, or None for one hydra's finder does
    /// not offer: 5, U.S. federally funded, and 8, National Science
    /// Foundation, which orrery's own sidebar had stopped offering too.
    ///
    /// orrery's 7 matched the projects *inside* the USA, the same as its 6;
    /// a link asking for 7 was asking for what the sidebar called it.
    fn from_id(id: i64) -> Option<Self> {
        Some(match id {
            1 => OrreryFilter::Online,
            2 => OrreryFilter::Misc(MiscFilter::HasClassroomMaterials),
            3 => OrreryFilter::Misc(MiscFilter::HasApp),
            4 => OrreryFilter::Misc(MiscFilter::IsAffiliate),
            6 => OrreryFilter::Misc(MiscFilter::UsaOnly),
            7 => OrreryFilter::Misc(MiscFilter::OutsideUsa),
            9 => OrreryFilter::Misc(MiscFilter::ExclusivelyOutdoors),
            10 => OrreryFilter::Misc(MiscFilter::ZooniverseAffiliate),
            99 => OrreryFilter::AnyStatus,
            _ => return None,
        })
    }
}

/// The orrery parameters of a link, read the way orrery's finder read them.
#[derive(Debug, Default)]
struct OrreryLink {
    phrase: String,
    location: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
    activity: Vec<String>,
    topic: Vec<String>,
    audience: Vec<String>,
    filters: Vec<OrreryFilter>,
    events: bool,
    projects: bool,
    active: bool,
}

impl OrreryLink {
    /// Read `params`, the pairs whose keys are orrery's.
    ///
    /// A single-valued parameter takes its first value, and a switch is on
    /// unless it is given as something other than `true`, as in orrery. A
    /// choice may be named by orrery's id or by its exact name, as older
    /// links name them; one naming nothing is dropped.
    fn read(params: &[(OrreryParam, String)]) -> Self {
        let first = |wanted: OrreryParam| {
            params
                .iter()
                .find(|(param, _)| *param == wanted)
                .map(|(_, value)| value.as_str())
        };
        let all = |wanted: OrreryParam| {
            params
                .iter()
                .filter(move |(param, _)| *param == wanted)
                .map(|(_, value)| value.as_str())
        };
        let switch = |wanted: OrreryParam| match first(wanted) {
            None | Some("") => true,
            Some(value) => value == "true",
        };
        let coordinate = |wanted: OrreryParam| {
            first(wanted)
                .and_then(|value| value.trim().parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value != 0.0)
        };
        let choices = |wanted: OrreryParam, choices: &[(i64, &'static str)]| {
            let mut named: Vec<String> = Vec::new();

            for value in all(wanted) {
                let value = match value.trim().parse::<i64>() {
                    Ok(id) => serde_json::Value::from(id),
                    Err(_) => serde_json::Value::from(value),
                };

                if let Some(name) = choice_ids::named(choices, &value)
                    && !named.iter().any(|held| held == name)
                {
                    named.push(name.to_string());
                }
            }

            named
        };

        OrreryLink {
            phrase: first(OrreryParam::Phrase)
                .unwrap_or_default()
                .trim()
                .to_string(),
            location: first(OrreryParam::Location)
                .unwrap_or_default()
                .trim()
                .to_string(),
            latitude: coordinate(OrreryParam::Latitude),
            longitude: coordinate(OrreryParam::Longitude),
            activity: choices(OrreryParam::Activity, choice_ids::ACTIVITIES),
            topic: choices(OrreryParam::Topic, choice_ids::TOPICS),
            audience: choices(OrreryParam::Audience, choice_ids::AUDIENCE),
            filters: all(OrreryParam::Filters)
                .filter_map(|value| value.trim().parse::<i64>().ok())
                .filter_map(OrreryFilter::from_id)
                .collect(),
            events: switch(OrreryParam::Events),
            projects: switch(OrreryParam::Projects),
            active: switch(OrreryParam::Active),
        }
    }

    /// The point the link names, when it gives one.
    fn point(&self) -> Option<(f64, f64)> {
        Some((self.latitude?, self.longitude?))
    }

    /// The hydra search asking what the link asked, given `native_q`, what the
    /// link's own `q` asks for, and `place`, the point its location names.
    ///
    /// A location the link names without a point is one orrery looked up
    /// before searching, which is what `place` is for. Without one, orrery
    /// matched the location's words against the projects' text, and so does
    /// this.
    ///
    /// orrery searched projects and events both unless told otherwise. A link
    /// that narrows projects in a way events have no part in -- by age group,
    /// activity, one of the numbered filters, or by taking in finished
    /// projects -- is a link about projects, and searches projects alone; in
    /// a search of both kinds hydra's finder would set those filters aside.
    fn finder_query(&self, native_q: &str, place: Option<(f64, f64)>) -> FinderQuery {
        let mut words = vec![native_q.trim(), self.phrase.as_str()];
        let mut query = FinderQuery::default();

        match place {
            Some((lat, lng)) => {
                query.lat = Some(lat);
                query.lng = Some(lng);
                query.location_name = Some(self.location.clone()).filter(|name| !name.is_empty());
            }
            None => words.push(self.location.as_str()),
        }

        let words: Vec<&str> = words.into_iter().filter(|w| !w.is_empty()).collect();
        query.q = Some(words.join(" ")).filter(|q| !q.is_empty());

        query.topic = self.topic.clone();
        query.audience = self.audience.clone();
        query.activity = self.activity.clone();

        for filter in &self.filters {
            match filter {
                OrreryFilter::Misc(misc) => {
                    if !query.has_misc_filter(*misc) {
                        query = query.with_misc_filter(*misc);
                    }
                }
                OrreryFilter::Online => query.location_type = Some("remote".to_string()),
                OrreryFilter::AnyStatus => query.show_completed = Some("on".to_string()),
            }
        }

        if !self.active {
            query.show_completed = Some("on".to_string());
        }

        let kinds = match (self.projects, self.events) {
            (true, true) if query.narrows_projects_only() => SearchKinds::ProjectsOnly,
            (true, true) => SearchKinds::Both,
            (true, false) => SearchKinds::ProjectsOnly,
            (false, true) => SearchKinds::EventsOnly,
            (false, false) => SearchKinds::Neither,
        };
        query.kind = kinds
            .as_params()
            .iter()
            .map(|kind| kind.to_string())
            .collect();

        query
    }
}

/// A query string split into the orrery parameters it carries and the rest.
struct SplitQuery<'a> {
    orrery: Vec<(OrreryParam, String)>,

    /// Each segment that is not an orrery parameter, as it was written, with
    /// its decoded key.
    rest: Vec<(String, &'a str)>,
}

impl<'a> SplitQuery<'a> {
    fn new(query: &'a str) -> Self {
        let mut split = SplitQuery {
            orrery: Vec::new(),
            rest: Vec::new(),
        };

        for segment in query.split('&').filter(|segment| !segment.is_empty()) {
            let (key, value) = form_urlencoded::parse(segment.as_bytes())
                .into_owned()
                .next()
                .unwrap_or_default();

            match OrreryParam::from_key(&key) {
                Some(param) => split.orrery.push((param, value)),
                None => split.rest.push((key, segment)),
            }
        }

        split
    }

    /// The link's orrery parameters, if it carries any.
    fn link(&self) -> Option<OrreryLink> {
        (!self.orrery.is_empty()).then(|| OrreryLink::read(&self.orrery))
    }

    /// The finder URL asking what `link`, read from this query, asks, with
    /// its location found at `place`.
    fn redirect(&self, link: &OrreryLink, place: Option<(f64, f64)>) -> String {
        let native_q = self.native("q").unwrap_or_default();

        self.url(&link.finder_query(&native_q, place))
    }

    /// The first value of `key` among the rest.
    fn native(&self, key: &str) -> Option<String> {
        self.rest
            .iter()
            .find(|(held, _)| held == key)
            .and_then(|(_, segment)| form_urlencoded::parse(segment.as_bytes()).next())
            .map(|(_, value)| value.into_owned())
    }

    /// The finder URL running `translated` and carrying the rest. Where the
    /// two give a single-valued parameter, the translation's stands; a list
    /// parameter holds the values of both.
    fn url(&self, translated: &FinderQuery) -> String {
        let translated = translated.to_query_string();
        let given: Vec<String> = form_urlencoded::parse(translated.as_bytes())
            .map(|(key, _)| key.into_owned())
            .collect();

        let mut segments: Vec<&str> = self
            .rest
            .iter()
            .filter(|(key, _)| key.ends_with("[]") || !given.contains(key))
            .map(|(_, segment)| *segment)
            .collect();
        segments.extend(translated.split('&').filter(|segment| !segment.is_empty()));

        let mut unique: Vec<&str> = Vec::new();
        for segment in segments {
            if !unique.contains(&segment) {
                unique.push(segment);
            }
        }

        if unique.is_empty() {
            "/finder".to_string()
        } else {
            format!("/finder?{}", unique.join("&"))
        }
    }
}

/// Where a finder request with query string `query` belongs, if it carries
/// orrery's parameters: the same search in hydra's.
pub(crate) async fn redirect_target(query: &str) -> Option<String> {
    let split = SplitQuery::new(query);
    let link = split.link()?;

    let place = match link.point() {
        Some(point) => Some(point),
        None if !link.location.is_empty() => locate(&[link.location.as_str()])
            .await
            .map(|point| (point.latitude, point.longitude)),
        None => None,
    };

    Some(split.redirect(&link, place))
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    /// The URL `query` is sent on to, given that its location, if it names
    /// one without a point, is found at `place`.
    fn translated(query: &str, place: Option<(f64, f64)>) -> Option<String> {
        let split = SplitQuery::new(query);
        let link = split.link()?;
        let place = link.point().or(place);

        Some(split.redirect(&link, place))
    }

    #[test]
    fn a_hydra_link_is_left_alone() {
        check!(translated("q=bats&topic[]=Birds&kind[]=project", None) == None);
        check!(translated("topic%5B%5D=Birds", None) == None);
        check!(translated("", None) == None);
    }

    /// The link on the campaign pages, which until now listed every project
    /// and every upcoming event besides: orrery's sidebar had no way to
    /// apply a project's filter to an event, and neither has hydra's.
    #[test]
    fn affiliate_projects() {
        check!(
            translated("projects=true&active=true&filters=4", None).as_deref()
                == Some("/finder?misc_filter[]=is_affiliate&kind[]=project")
        );
    }

    #[test]
    fn a_search_near_a_place_keeps_the_campaign_it_came_from() {
        check!(
            translated(
                "events=true&projects=true&longitude=-75.165222&latitude=39.952583&active=true&back=philly&utm_source=x",
                None,
            )
            .as_deref()
                == Some("/finder?back=philly&utm_source=x&lat=39.952583&lng=-75.165222")
        );
    }

    #[test]
    fn choices_are_named_by_id_or_by_name() {
        check!(
            translated(
                "filters=2&audience=111&audience=3&audience=111&topic=Climate+%26+Weather&activity=28",
                None
            )
            .as_deref()
                == Some(
                    "/finder?topic[]=Climate%20%26%20Weather&audience[]=Elementary%20school%20%286%20-%2010%20years%29\
                     &activity[]=On%20a%20lunch%20break&misc_filter[]=has_classroom_materials&kind[]=project"
                )
        );
    }

    #[test]
    fn a_phrase_is_searched_for_with_any_words_the_link_already_has() {
        check!(
            translated("phrase=Instant+Wild&projects=true&active=false", None).as_deref()
                == Some("/finder?q=Instant%20Wild&show_completed=on&kind[]=project")
        );
        check!(
            translated("q=bats&phrase=caves", None).as_deref() == Some("/finder?q=bats%20caves")
        );
    }

    #[test]
    fn a_place_named_without_a_point_is_looked_up() {
        check!(
            translated("location=Philadelphia%2C+PA", Some((39.95, -75.17))).as_deref()
                == Some("/finder?lat=39.95&lng=-75.17&location_name=Philadelphia%2C%20PA")
        );
    }

    #[test]
    fn a_place_that_cannot_be_found_is_searched_for_by_name() {
        check!(
            translated("phrase=birds&location=Nowhere", None).as_deref()
                == Some("/finder?q=birds%20Nowhere")
        );
    }

    #[test]
    fn the_point_a_link_gives_replaces_an_empty_one() {
        check!(
            translated("lat=&lng=&latitude=40.5&longitude=-74.5", None).as_deref()
                == Some("/finder?lat=40.5&lng=-74.5")
        );
    }

    #[test]
    fn online_only_applies_to_events_as_well() {
        check!(translated("filters=1", None).as_deref() == Some("/finder?location_type=remote"));
    }

    #[test]
    fn events_alone_or_nothing() {
        check!(
            translated("events=true&projects=false", None).as_deref()
                == Some("/finder?kind[]=event")
        );
        check!(
            translated("events=false&projects=false", None).as_deref()
                == Some("/finder?kind[]=none")
        );
    }

    /// What hydra's finder cannot ask is dropped rather than carried, or the
    /// translated link would be translated again, forever.
    #[test]
    fn a_translated_link_is_never_translated_again() {
        for query in [
            "filters=5&filters=8&topic=999&audience=nonsense&active=true",
            "projects=true&active=true&filters=4&tag=nasa&SDG=SDG_14",
            "phrase=&location=&latitude=&longitude=&activity=&topic=",
        ] {
            let url = translated(query, None).unwrap();
            let again = url.split_once('?').map(|(_, query)| query).unwrap_or("");

            check!(translated(again, None) == None, "{url}");
        }
    }

    #[test]
    fn what_hydra_cannot_ask_is_dropped() {
        check!(
            translated("filters=5&filters=8&topic=999&tag=nasa&SDG=SDG_14", None).as_deref()
                == Some("/finder?tag=nasa&SDG=SDG_14")
        );
    }
}
