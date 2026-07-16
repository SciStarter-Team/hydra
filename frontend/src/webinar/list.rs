//! The filtered webinar list: a page-builder widget.
//!
//! Staff place a component whose source declares a `groups` param and carries
//! `{$^ widget webinar_list groups="*groups" $}`. Each page picks its own filter
//! vocabulary — the library page speaks of "Professional Development" and "For
//! the Public", /nasa-live groups the same tags differently — while a webinar is
//! tagged only once.

use std::sync::Arc;

use anyhow::Error;
use futures::future::BoxFuture;
use sailfish::TemplateOnce;
use serde::Deserialize;

use crate::builder::context::{Context, RequestUrl};
use crate::builder::widget::{Widget, WidgetParams};
use crate::register_widget;
use crate::webinar::{Webinar, WebinarLink};

/// The id of the region HTMX swaps.
///
/// A page is expected to carry at most one webinar list: the filter selections
/// live in the page's `tag` parameters, so two lists would necessarily show the
/// same thing. That is a stated constraint, not an oversight.
const LIST_ID: &str = "webinar-list";

/// One "Pick From:" dropdown.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterGroup {
    pub label: String,
    pub values: Vec<String>,
}

impl FilterGroup {
    /// A DOM-safe form of the label, for tying a dropdown to its own label.
    ///
    /// Staff write these labels, so they may contain spaces or punctuation that
    /// have no business in an id attribute.
    pub fn slug(&self) -> String {
        self.label
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect()
    }

    /// Whichever of the reader's selections belongs to this group, if any.
    fn selection<'a>(&self, selected: &'a [String]) -> Option<&'a str> {
        selected
            .iter()
            .find(|tag| {
                self.values
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(tag))
            })
            .map(String::as_str)
    }
}

/// Reads the per-page filter vocabulary.
///
/// The format is one group per `|`, its label before the `:`, its values comma
/// separated after it:
///
/// ```text
/// Audience: Professional Development, For the Public
///   | Topic: Animals, Pollinators, Space
///   | Age: Youth, Teens, Adults
/// ```
///
/// Deliberately quote-free and newline-tolerant: the spec is interpolated into
/// a `{$ widget $}` directive when the component is placed, and a quote in the
/// value would end the attribute early.
pub fn parse_groups(spec: &str) -> Vec<FilterGroup> {
    spec.split('|')
        .filter_map(|group| {
            let (label, values) = group.split_once(':')?;

            let values: Vec<String> = values
                .split(',')
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect();

            let label = label.trim().to_string();

            if label.is_empty() || values.is_empty() {
                return None;
            }

            Some(FilterGroup { label, values })
        })
        .collect()
}

/// The reader's selections, as they arrive in the URL.
///
/// Every dropdown submits under the same `tag[]` name, so the query reads
/// `?tag[]=For+the+Public&tag[]=Pollinators` — shareable, which is the point of
/// keeping the state here rather than in the DOM. The brackets are not
/// decoration: `serde_qs` only collects repeated parameters into a list in that
/// form, and it is the same convention the finder's `topic[]` already uses.
#[derive(Debug, Default, Deserialize)]
pub struct FilterSelections {
    #[serde(default)]
    pub tag: Vec<String>,
}

impl FilterSelections {
    pub fn from_query(query: &str) -> FilterSelections {
        serde_qs::Config::new(5, false)
            .deserialize_str::<FilterSelections>(query)
            .unwrap_or_default()
    }

    /// The selections that actually narrow anything.
    pub fn active(&self) -> Vec<String> {
        self.tag
            .iter()
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect()
    }
}

/// Keeps only the webinars carrying every selected tag.
///
/// Selections from different groups intersect — "For the Public" *and*
/// "Pollinators" — which is what makes stacked dropdowns narrow rather than
/// widen.
fn matching(webinars: Vec<Webinar>, selected: &[String]) -> Vec<Webinar> {
    webinars
        .into_iter()
        .filter(|webinar| selected.iter().all(|tag| webinar.has_tag(tag)))
        .collect()
}

/// A webinar as the list draws it.
pub struct WebinarCard {
    pub title: String,
    pub description: String,
    pub image: String,
    pub image_credit: String,
    pub platform: String,
    pub date: String,
    pub upcoming: bool,
    pub featured_entity_slug: String,
    pub links: Vec<WebinarLink>,
}

impl WebinarCard {
    fn new(webinar: Webinar) -> WebinarCard {
        WebinarCard {
            date: format_date(&webinar.held_at),
            upcoming: webinar.is_upcoming(),
            title: webinar.title,
            description: webinar.description,
            image: webinar.image,
            image_credit: webinar.image_credit,
            platform: webinar.platform,
            featured_entity_slug: webinar.featured_entity_slug,
            links: webinar.links,
        }
    }

    pub fn featured_entity_path(&self) -> String {
        format!("/{}", self.featured_entity_slug)
    }
}

/// Renders the date the way both pages show it — "APR 29", "JUN 17 2026".
///
/// The year is dropped for the current year, since an archive of this year's
/// webinars repeating "2026" on every card is noise.
fn format_date(held_at: &common::timestring::TimeString) -> String {
    let Ok(timestamp): Result<jiff::Timestamp, _> = held_at.clone().try_into() else {
        return String::new();
    };

    let Ok(zone) = jiff::tz::TimeZone::get("US/Eastern") else {
        return String::new();
    };

    let held = timestamp.to_zoned(zone);
    let this_year = jiff::Timestamp::now()
        .to_zoned(held.time_zone().clone())
        .year();

    if held.year() == this_year {
        held.strftime("%b %-d").to_string().to_uppercase()
    } else {
        held.strftime("%b %-d %Y").to_string().to_uppercase()
    }
}

#[derive(TemplateOnce)]
#[template(path = "site/webinar_card.stpl.html")]
pub struct WebinarCardTemplate<'a> {
    pub webinar: &'a WebinarCard,
}

#[derive(TemplateOnce)]
#[template(path = "site/webinar_list.stpl.html")]
struct WebinarListTemplate {
    list_id: &'static str,
    action: String,
    groups: Vec<FilterGroup>,
    selected: Vec<String>,
    upcoming: Vec<WebinarCard>,
    archived: Vec<WebinarCard>,
    filtered: bool,
}

impl WebinarListTemplate {
    /// The value a group's dropdown should show as chosen.
    fn selection_for(&self, group: &FilterGroup) -> &str {
        group.selection(&self.selected).unwrap_or("")
    }

    fn is_empty(&self) -> bool {
        self.upcoming.is_empty() && self.archived.is_empty()
    }
}

pub struct WebinarList;

impl Widget for WebinarList {
    fn name(&self) -> &'static str {
        "webinar_list"
    }

    fn label(&self) -> &'static str {
        "Filtered Webinar List"
    }

    fn render<'a>(
        &'a self,
        ctx: &'a Arc<Context>,
        params: WidgetParams<'a>,
    ) -> BoxFuture<'a, Result<String, Error>> {
        Box::pin(async move {
            let url: RequestUrl = ctx.get()?.unwrap_or_default();
            let groups = parse_groups(params.get_or_empty("groups"));
            let selected = FilterSelections::from_query(&url.query).active();

            let webinars = matching(Webinar::list_published().await?, &selected);

            // Both pages lead with what is still to come and follow with the
            // archive, so the split is the list's job rather than the reader's.
            let (upcoming, archived): (Vec<Webinar>, Vec<Webinar>) =
                webinars.into_iter().partition(Webinar::is_upcoming);

            let template = WebinarListTemplate {
                list_id: LIST_ID,
                action: url.path,
                groups,
                filtered: !selected.is_empty(),
                selected,
                // list_published sorts newest first, which reads backwards for
                // things that have not happened yet.
                upcoming: upcoming.into_iter().rev().map(WebinarCard::new).collect(),
                archived: archived.into_iter().map(WebinarCard::new).collect(),
            };

            Ok(template.render_once()?)
        })
    }
}

register_widget!(WebinarList);

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;
    use common::insensitivestring::InsensitiveString;

    fn webinar(tags: &[&str]) -> Webinar {
        Webinar {
            tags: tags
                .iter()
                .map(|tag| InsensitiveString::from(tag.to_string()))
                .collect(),
            ..Webinar::default()
        }
    }

    #[test]
    fn groups_parse_into_dropdowns() {
        let groups = parse_groups(
            "Audience: Professional Development, For the Public \
             | Topic: Animals, Pollinators, Space \
             | Age: Youth, Teens",
        );

        check!(groups.len() == 3);
        check!(groups[0].label == "Audience");
        check!(groups[0].values == vec!["Professional Development", "For the Public"]);
        check!(groups[2].values == vec!["Youth", "Teens"]);
    }

    /// Staff edit this in a textarea, so it arrives however they typed it.
    #[test]
    fn groups_tolerate_the_whitespace_a_textarea_produces() {
        let groups =
            parse_groups("  Audience :  For the Public ,, Youth  \n | \n Topic: Animals \n");

        check!(groups.len() == 2);
        check!(groups[0].label == "Audience");
        check!(groups[0].values == vec!["For the Public", "Youth"]);
        check!(groups[1].values == vec!["Animals"]);
    }

    #[test]
    fn a_malformed_group_is_skipped_rather_than_breaking_the_page() {
        let groups = parse_groups("no colon here | Topic: Animals | Empty: | : orphaned");

        check!(groups.len() == 1);
        check!(groups[0].label == "Topic");
    }

    #[test]
    fn an_absent_spec_yields_no_dropdowns() {
        check!(parse_groups("").is_empty());
    }

    #[test]
    fn selections_come_from_repeated_tag_parameters() {
        let selections =
            FilterSelections::from_query("tag[]=For+the+Public&tag[]=&tag[]=Pollinators");

        check!(selections.active() == vec!["For the Public", "Pollinators"]);
    }

    #[test]
    fn an_unfiltered_page_selects_nothing() {
        check!(FilterSelections::from_query("").active().is_empty());
    }

    /// Stacked dropdowns have to narrow. If these unioned, picking a topic would
    /// bring back webinars the audience filter had just excluded.
    #[test]
    fn selections_from_different_groups_intersect() {
        let webinars = vec![
            webinar(&["For the Public", "Pollinators"]),
            webinar(&["For the Public", "Space"]),
            webinar(&["Professional Development", "Pollinators"]),
        ];

        let selected = vec!["For the Public".to_string(), "Pollinators".to_string()];

        check!(matching(webinars, &selected).len() == 1);
    }

    #[test]
    fn no_selection_keeps_everything() {
        let webinars = vec![webinar(&["Animals"]), webinar(&[])];

        check!(matching(webinars, &[]).len() == 2);
    }

    /// Emma's vocabulary is title-cased prose; nobody should have to match its
    /// capitalisation to get a hit.
    #[test]
    fn tag_matching_ignores_case() {
        let webinars = vec![webinar(&["For the Public"])];

        check!(matching(webinars, &["for the public".to_string()]).len() == 1);
    }

    /// Placing the shipped component must leave the directive behind, with the
    /// staff-chosen groups interpolated into it — not the widget's output, and
    /// not a mangled directive.
    ///
    /// This exercises the real `components/webinar-list.json`, so a typo in its
    /// escape syntax fails here rather than on the page.
    #[tokio::test]
    async fn placing_the_shipped_component_bakes_a_live_directive() {
        use crate::builder::context::ContextMode;
        use crate::builder::render::CompiledTemplate;
        use crate::pagebuilder::PageBuilderComponent;
        use sqlx::SqlitePool;

        let json = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("components/webinar-list.json"),
        )
        .expect("read webinar-list.json");

        let component: PageBuilderComponent =
            serde_json::from_str(&json).expect("parse webinar-list.json");

        let pool = Box::leak(Box::new(SqlitePool::connect(":memory:").await.unwrap()));
        let ctx = Context::new(pool, &None, ContextMode::Presenting).unwrap();

        // What compose_component_into_slot does when staff drop the component.
        ctx.bind_string("groups", "Audience: For the Public | Topic: Pollinators")
            .unwrap();

        let placed = CompiledTemplate::compile(&component.source)
            .unwrap()
            .render(&ctx)
            .await
            .unwrap();

        check!(
            placed.trim()
                == r#"{$ widget webinar_list groups="Audience: For the Public | Topic: Pollinators" $}"#
        );

        // And what it baked is a directive the page can actually run.
        let recompiled = CompiledTemplate::compile(placed.trim());
        check!(recompiled.is_ok());
        check!(crate::builder::widget::lookup("webinar_list").is_some());
    }

    #[test]
    fn a_dropdown_shows_the_selection_that_belongs_to_it() {
        let group = FilterGroup {
            label: "Topic".to_string(),
            values: vec!["Animals".to_string(), "Space".to_string()],
        };

        let selected = vec!["For the Public".to_string(), "Space".to_string()];

        check!(group.selection(&selected) == Some("Space"));
        check!(group.selection(&["For the Public".to_string()]) == None);
    }
}
