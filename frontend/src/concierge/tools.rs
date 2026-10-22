//! Tool definitions and dispatch for the concierge.
//!
//! The model's only path to catalog information is through tools:
//! `search_projects` runs the same query engine as the `/finder` page
//! (covering projects and upcoming events), and `list_communities` lists the
//! public communities. The model sees compact, index-keyed digests of the
//! results; the full rows stay server-side and are the only source for
//! rendered cards, so hallucinated projects, events, or communities are
//! structurally impossible.
//!
//! Site navigation works the same way: the system prompt lists a curated set
//! of destination URLs, and [`render_assistant_html`] turns exactly those
//! URLs — nothing else — into anchors while interpreting the model's light
//! markdown with raw HTML escaped and foreign link targets stripped.
//!
//! What Flower may say about SciStarter *itself* is curated on the same
//! terms. Destinations are grouped into [`ProgramArea`]s, each carrying a
//! staff-written note on SciStarter's role in that area, and the model is
//! told to reproduce the note rather than compose one — so the partners,
//! programs, and numbers in a "how this relates to SciStarter" line come
//! from this file, not from what a general-purpose model believes about the
//! organization.

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::Error;
use aws_sdk_bedrockruntime::types::{Tool, ToolConfiguration, ToolInputSchema, ToolSpecification};
use common::message::user::CommunityInfo;
use common::shared::db;

use crate::concierge::TOOL_RESULTS_TO_MODEL;
use crate::concierge::engine::json_to_document;
use crate::geocode::geocode_address_to_point;
use crate::search::{self, FinderQuery, FinderResult, FtsTermStyle, SearchKinds};

/// The most-used project tags, minus any that duplicate a facet value.
/// Tags are baked into the FTS summaries, so every one of these works as a
/// search keyword; surfacing them teaches the model the catalog's actual
/// vocabulary ("insects & pollinators", "long-term species monitoring")
/// instead of leaving it to guess.
async fn load_common_tags(
    topics: &[String],
    audiences: &[String],
    activities: &[String],
) -> Result<Vec<String>, Error> {
    let tags = sqlx::query_scalar!(
        r#"SELECT "tag" FROM "project_tags"
           GROUP BY "tag" ORDER BY COUNT(*) DESC, "tag" LIMIT 80"#
    )
    .fetch_all(db())
    .await?;

    let duplicates_facet = |tag: &String| {
        [topics, audiences, activities]
            .iter()
            .any(|list| list.iter().any(|facet| facet.eq_ignore_ascii_case(tag)))
    };

    Ok(tags
        .into_iter()
        .filter(|tag| !duplicates_facet(tag))
        .take(60)
        .collect())
}

/// Where a curated destination's availability comes from.
enum DestinationSource {
    /// A route compiled into the frontend — always available.
    Native,
    /// A content-managed page: available once a published entity exists at
    /// this slug. Pages still being ported from the old site stay out of the
    /// prompt until their content arrives, so Flower never offers a page
    /// that would 404.
    Entity(&'static str),
    /// A page that lives on the WordPress site and has no page here yet.
    /// Always available, since its availability is not ours to gate. When one
    /// of these is ported, it becomes an `Entity` with a relative URL.
    External,
}

/// A body of SciStarter's own work that the site has curated pages for.
///
/// Each area carries a staff-written note on SciStarter's role in it. That
/// note is the only thing Flower may say about SciStarter's programs,
/// partners, or numbers: the model quotes it rather than composing from what
/// it happens to believe about the organization, which keeps organizational
/// claims as un-inventable as project and event details are.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProgramArea {
    Libraries,
    Education,
    GirlScouts,
    Nasa,
    Ambassadors,
    Training,
}

impl ProgramArea {
    /// The area's heading in the prompt.
    fn name(self) -> &'static str {
        match self {
            ProgramArea::Libraries => "Libraries",
            ProgramArea::Education => "Educators and students",
            ProgramArea::GirlScouts => "Girl Scouts",
            ProgramArea::Nasa => "NASA",
            ProgramArea::Ambassadors => "Ambassadors",
            ProgramArea::Training => "Trainings and badges",
        }
    }

    /// How SciStarter's work relates to the area. Reproduced in replies
    /// nearly verbatim, so it is written as visitor-facing copy.
    fn relates(self) -> &'static str {
        match self {
            ProgramArea::Libraries => {
                "SciStarter and Arizona State University run the Citizen and \
                 Community Science Library Network, which helps libraries and \
                 community organizations become community hubs for citizen \
                 science. It began in 2017 with six pilot libraries in Arizona \
                 and support from the Institute for Museum and Library \
                 Services, and now connects more than 1,200 libraries \
                 nationwide — with professional development, program and event \
                 resources, ready-to-circulate kits, and projects."
            }
            ProgramArea::Education => {
                "SciStarter supports parents, guardians, and both formal and \
                 informal educators with classroom and at-home STEM \
                 resources: projects sorted by grade level, free self-guided \
                 training modules, and lists teachers can use to share \
                 projects with a class and track participation."
            }
            ProgramArea::GirlScouts => {
                "SciStarter works with Girl Scouts on the Think Like a \
                 Citizen Scientist Journey, the Girl Scout Tree Promise, and \
                 the Climate Challenge, with meeting-by-meeting guidance for \
                 volunteers and curated projects Girl Scouts can complete to \
                 earn their awards."
            }
            ProgramArea::Nasa => {
                "SciStarter presents Do NASA Science LIVE! in partnership \
                 with NASA Citizen Science — a free series of virtual events \
                 featuring NASA citizen-science projects, live Q&As with \
                 scientists, and step-by-step instructions for joining in as \
                 the event happens."
            }
            ProgramArea::Ambassadors => {
                "As libraries and community organizations become hubs for \
                 citizen science, they need volunteers to help sustain the \
                 work. The SciStarter Ambassador Program gives those \
                 volunteers the knowledge, skills, and professionally \
                 designed resources to support citizen science where they \
                 live. Certified Ambassadors have earned the Foundations and \
                 Ambassador badges, been independently screened, and appear \
                 in a searchable directory that event hosts can invite from."
            }
            ProgramArea::Training => {
                "SciStarter's trainings are free and self-guided, built by \
                 the instructional designers at Arizona State University with \
                 SciStarter's own experts, and each one earns a digital badge \
                 that can go on a LinkedIn or professional profile. 94% of \
                 the people who finished Foundations of Citizen Science \
                 reported more confidence taking part in projects and working \
                 with data."
            }
        }
    }
}

/// A page Flower may direct visitors to. `url` is relative for pages on this
/// site and absolute for [`DestinationSource::External`] ones; either way it
/// is the exact text the model writes to produce a link.
struct Destination {
    url: &'static str,
    purpose: &'static str,
    /// The program area this page belongs to, if any. Pages with no area are
    /// general navigation, and carry no claim about SciStarter's work.
    area: Option<ProgramArea>,
    source: DestinationSource,
}

/// The curated navigation whitelist. These are the only paths that render as
/// links in assistant replies, and their order is the order the program areas
/// appear in the prompt.
const DESTINATIONS: &[Destination] = &[
    Destination {
        url: "/training/foundations",
        purpose: "the Foundations of Citizen Science training — the best \
                  first step for anyone new to citizen science",
        area: Some(ProgramArea::Training),
        source: DestinationSource::Native,
    },
    Destination {
        url: "/training",
        purpose: "free trainings that earn badges and certificates",
        area: Some(ProgramArea::Training),
        source: DestinationSource::Entity("training"),
    },
    Destination {
        url: "/dashboard",
        purpose: "personalized project recommendations for logged-in visitors",
        area: None,
        source: DestinationSource::Native,
    },
    Destination {
        url: "/signup",
        purpose: "creating a free SciStarter account",
        area: None,
        source: DestinationSource::Native,
    },
    Destination {
        url: "/education",
        purpose: "students and teachers",
        area: Some(ProgramArea::Education),
        source: DestinationSource::Entity("education"),
    },
    Destination {
        url: "/library",
        purpose: "the program overview for librarians and library staff",
        area: Some(ProgramArea::Libraries),
        source: DestinationSource::Entity("library"),
    },
    Destination {
        url: "/library-kits",
        purpose: "for the public: what a citizen-science kit holds, and the \
                  projects the kits support",
        area: Some(ProgramArea::Libraries),
        source: DestinationSource::Entity("library-kits"),
    },
    Destination {
        url: "/library-build-a-kit",
        purpose: "step-by-step guides for building and promoting citizen \
                  science kits in a library",
        area: Some(ProgramArea::Libraries),
        source: DestinationSource::Entity("library-build-a-kit"),
    },
    Destination {
        url: "https://pages.scistarter.org/library-network-about/",
        purpose: "what the Citizen and Community Science Library Network is, \
                  what it offers members, and how a library joins",
        area: Some(ProgramArea::Libraries),
        source: DestinationSource::External,
    },
    Destination {
        url: "https://pages.scistarter.org/library-network-locations/",
        purpose: "a map for finding a participating library nearby",
        area: Some(ProgramArea::Libraries),
        source: DestinationSource::External,
    },
    Destination {
        url: "https://pages.scistarter.org/library-program-resources/",
        purpose: "resources for library staff: webinars, self-guided \
                  trainings, and the Library & Community Guide to Citizen \
                  Science",
        area: Some(ProgramArea::Libraries),
        source: DestinationSource::External,
    },
    Destination {
        url: "/nasa",
        purpose: "NASA fans and space enthusiasts",
        area: Some(ProgramArea::Nasa),
        source: DestinationSource::Entity("nasa"),
    },
    Destination {
        url: "/girlscouts-info",
        purpose: "Girl Scouts and their leaders",
        area: Some(ProgramArea::GirlScouts),
        source: DestinationSource::Entity("girlscouts-info"),
    },
    Destination {
        url: "/girlscouts-journey",
        purpose: "the Think Like a Citizen Scientist Journey, meeting by \
                  meeting",
        area: Some(ProgramArea::GirlScouts),
        source: DestinationSource::Entity("girlscouts-journey"),
    },
    Destination {
        url: "/girlscouts-treepromise",
        purpose: "the Girl Scout Tree Promise and the projects that count \
                  toward it",
        area: Some(ProgramArea::GirlScouts),
        source: DestinationSource::Entity("girlscouts-treepromise"),
    },
    Destination {
        url: "/girlscouts-climate",
        purpose: "the Girl Scout Climate Challenge and its projects",
        area: Some(ProgramArea::GirlScouts),
        source: DestinationSource::Entity("girlscouts-climate"),
    },
    Destination {
        url: "/girlscouts-faq",
        purpose: "answers to common Girl Scout questions about SciStarter \
                  accounts and awards",
        area: Some(ProgramArea::GirlScouts),
        source: DestinationSource::Entity("girlscouts-faq"),
    },
    Destination {
        url: "/ambassador",
        purpose: "the SciStarter Ambassador program",
        area: Some(ProgramArea::Ambassadors),
        source: DestinationSource::Entity("ambassador"),
    },
    Destination {
        url: crate::ambassador::DIRECTORY_PATH,
        purpose: "the directory of SciStarter Ambassadors, searchable by location",
        area: Some(ProgramArea::Ambassadors),
        source: DestinationSource::Native,
    },
    Destination {
        url: "/communities",
        purpose: "browsing all public communities",
        area: None,
        source: DestinationSource::Native,
    },
    Destination {
        url: "/finder",
        purpose: "the full project finder",
        area: None,
        source: DestinationSource::Native,
    },
];

/// The destinations currently reachable: native routes and WordPress pages,
/// plus any entity-backed pages whose content has been published here.
/// Degrades to the ungated ones only if the entity lookup fails.
async fn available_destinations() -> Vec<&'static Destination> {
    let slugs: Vec<&'static str> = DESTINATIONS
        .iter()
        .filter_map(|d| match d.source {
            DestinationSource::Entity(slug) => Some(slug),
            DestinationSource::Native | DestinationSource::External => None,
        })
        .collect();

    let published: Vec<String> = if slugs.is_empty() {
        Vec::new()
    } else {
        let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
            r#"SELECT "slug" FROM "entities"
               WHERE "publish" = 1
               AND "kind" NOT IN ('Project', 'Event', 'Placeholder')
               AND "slug" IN ("#,
        );
        let mut separated = qb.separated(", ");
        for slug in &slugs {
            separated.push_bind(*slug);
        }
        qb.push(")");
        match qb.build_query_scalar().fetch_all(db()).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("Concierge destination lookup failed: {}", e);
                Vec::new()
            }
        }
    };

    DESTINATIONS
        .iter()
        .filter(|d| match d.source {
            DestinationSource::Native | DestinationSource::External => true,
            DestinationSource::Entity(slug) => published.iter().any(|p| p == slug),
        })
        .collect()
}

/// The URLs eligible for linkification in assistant replies.
pub async fn linkable_urls() -> Vec<&'static str> {
    available_destinations()
        .await
        .iter()
        .map(|d| d.url)
        .collect()
}

/// Split available destinations into the program areas that still have at
/// least one reachable page — in whitelist order, so the prompt is stable —
/// and the general-navigation pages that belong to no area. An area whose
/// pages are all unpublished simply doesn't appear, taking its note with it.
fn group_by_area(
    destinations: &[&'static Destination],
) -> (
    Vec<(ProgramArea, Vec<&'static Destination>)>,
    Vec<&'static Destination>,
) {
    let mut areas: Vec<(ProgramArea, Vec<&'static Destination>)> = Vec::new();
    let mut general = Vec::new();

    for destination in destinations {
        match destination.area {
            Some(area) => match areas.iter_mut().find(|(known, _)| *known == area) {
                Some((_, pages)) => pages.push(destination),
                None => areas.push((area, vec![destination])),
            },
            None => general.push(*destination),
        }
    }

    (areas, general)
}

/// Render the destination half of the system prompt: one block per program
/// area — its name, its "how this relates to SciStarter" note, and its pages
/// — followed by the general-navigation pages.
fn destination_sections(destinations: &[&'static Destination]) -> String {
    fn page_line(destination: &Destination) -> String {
        format!("- {} — {}\n", destination.url, destination.purpose)
    }

    let (areas, general) = group_by_area(destinations);
    let mut sections = String::new();

    for (area, pages) in &areas {
        sections.push_str(&format!(
            "{}\nHow this relates to SciStarter: {}\nPages:\n{}\n",
            area.name(),
            area.relates(),
            pages.iter().map(|page| page_line(page)).collect::<String>(),
        ));
    }

    sections.push_str("General site pages:\n");
    for destination in &general {
        sections.push_str(&page_line(destination));
    }

    sections
}

/// Whether a byte continues a word or a URL path, and so may not sit
/// against either end of a match.
fn word_like(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'/')
}

/// A URL without its trailing slash, for comparisons that treat one as
/// optional. The WordPress destinations are canonically written with a
/// trailing slash and the model drops it about as often as it keeps it;
/// losing the link over that helps nobody, and both forms reach the same
/// page.
fn without_trailing_slash(url: &str) -> &str {
    url.strip_suffix('/').unwrap_or(url)
}

/// The whitelisted URL a link destination refers to, if any, treating a
/// trailing slash as optional.
fn whitelisted(destination: &str, urls: &[&'static str]) -> Option<&'static str> {
    let destination = without_trailing_slash(destination);
    urls.iter()
        .copied()
        .find(|url| without_trailing_slash(url) == destination)
}

/// A piece of assistant text after URL matching: literal text, or a
/// whitelisted destination URL.
enum Segment {
    Text(String),
    Url(&'static str),
}

/// Split `text` on occurrences of the whitelisted `urls`. A match must stand
/// alone: the characters on either side may not be word- or path-like,
/// so "/training" never matches inside "/training/foundations" or
/// "foo/training". Longest path wins at any position.
fn split_urls(text: &str, urls: &[&'static str]) -> Vec<Segment> {
    /// The bytes of `rest` that `url` matches, or `None` if it doesn't
    /// stand alone there. The trailing slash is optional on both sides.
    fn match_len(rest: &str, url: &str) -> Option<usize> {
        let bare = without_trailing_slash(url);
        if !rest.starts_with(bare) {
            return None;
        }
        let len = match rest.as_bytes().get(bare.len()) {
            Some(b'/') => bare.len() + 1,
            _ => bare.len(),
        };
        match rest.as_bytes().get(len) {
            Some(&next) if word_like(next) => None,
            _ => Some(len),
        }
    }

    let mut ordered: Vec<&'static str> = urls.to_vec();
    ordered.sort_by_key(|url| std::cmp::Reverse(url.len()));

    // Destination URLs are ASCII, so byte offsets used for matching always
    // land on character boundaries.
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let preceded_by_word = at > 0 && word_like(text.as_bytes()[at - 1]);
        let hit = (!preceded_by_word)
            .then(|| {
                ordered
                    .iter()
                    .find_map(|url| match_len(rest, url).map(|len| (*url, len)))
            })
            .flatten();
        match hit {
            // The canonical whitelist form is what gets linked, whichever
            // form the model wrote.
            Some((url, len)) => {
                if !literal.is_empty() {
                    segments.push(Segment::Text(std::mem::take(&mut literal)));
                }
                segments.push(Segment::Url(url));
                at += len;
            }
            None => {
                let ch = rest.chars().next().expect("non-empty remainder");
                literal.push(ch);
                at += ch.len_utf8();
            }
        }
    }
    if !literal.is_empty() {
        segments.push(Segment::Text(literal));
    }
    segments
}

/// The anchor markup for a whitelisted destination URL.
fn url_anchor(url: &str) -> String {
    format!(r#"<a href="{url}">{url}</a>"#)
}

/// Render assistant output as sanitized HTML. Haiku emits light markdown
/// despite prompting, so the text is interpreted as markdown rather than
/// shown with literal `**` markers — but on the model's-output-is-untrusted
/// terms:
///
/// - raw HTML becomes escaped text (`push_html` escapes `Text` events);
/// - markdown links are honored only when the destination is a whitelisted
///   URL, up to a trailing slash — any other link is unwrapped to its inner
///   text, and images are dropped;
/// - bare whitelisted URLs in ordinary text become anchors;
/// - soft line breaks render as real breaks, chat-style.
///
/// This remains the only route from model output to markup, and it can only
/// produce links to the curated destinations.
pub fn render_assistant_html(text: &str, urls: &[&'static str]) -> String {
    use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};

    let mut events: Vec<Event> = Vec::new();
    // Non-whitelisted links are unwrapped: the stack records, per open link,
    // whether its end tag should be emitted.
    let mut link_kept = Vec::new();
    let mut image_depth = 0usize;
    let mut code_depth = 0usize;

    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Image { .. }) => image_depth += 1,
            Event::End(TagEnd::Image) => image_depth -= 1,
            // Drop images entirely, alt text included.
            _ if image_depth > 0 => {}
            Event::Start(Tag::Link { dest_url, .. }) => {
                let target = whitelisted(&dest_url, urls);
                if let Some(url) = target {
                    events.push(Event::Start(Tag::Link {
                        link_type: LinkType::Inline,
                        dest_url: url.into(),
                        title: "".into(),
                        id: "".into(),
                    }));
                }
                link_kept.push(target.is_some());
            }
            Event::End(TagEnd::Link) => {
                if link_kept.pop().unwrap_or(false) {
                    events.push(Event::End(TagEnd::Link));
                }
            }
            Event::Start(tag @ Tag::CodeBlock(_)) => {
                code_depth += 1;
                events.push(Event::Start(tag));
            }
            Event::End(TagEnd::CodeBlock) => {
                code_depth -= 1;
                events.push(Event::End(TagEnd::CodeBlock));
            }
            // Linkify bare URLs in ordinary text — not inside an existing
            // link or a code block.
            Event::Text(text) if link_kept.is_empty() && code_depth == 0 => {
                for segment in split_urls(&text, urls) {
                    match segment {
                        Segment::Text(literal) => events.push(Event::Text(literal.into())),
                        Segment::Url(url) => events.push(Event::Html(url_anchor(url).into())),
                    }
                }
            }
            // Raw HTML from the model renders as escaped text.
            Event::Html(html) => events.push(Event::Text(html)),
            Event::InlineHtml(html) => events.push(Event::Text(html)),
            // Chat-style newlines: a single newline is a real break.
            Event::SoftBreak => events.push(Event::HardBreak),
            other => events.push(other),
        }
    }

    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    html
}

/// Build the system prompt. The vocabulary queries are ordered, so the
/// output is deterministic for identical database contents. No timestamps or
/// per-user data belong here; per-request context is prefixed to the first
/// user message instead.
pub async fn system_prompt() -> Result<String, Error> {
    let (topics, audiences, activities) = search::load_filter_options().await?;
    let common_tags = load_common_tags(&topics, &audiences, &activities).await?;
    let sections = destination_sections(&available_destinations().await);

    Ok(format!(
        "You are Flower, the SciStarter concierge — a smart, loyal, playful, \
         and adventurous dog who is extremely intuitive about what visitors \
         are looking for, and helps them find real citizen-science projects, \
         upcoming events, and public communities they can join. Let a little \
         canine charm show (warmth, enthusiasm, an eager nose for discovery), \
         but keep it light: at most one playful touch per reply, and \
         helpfulness always comes first.\n\
         \n\
         Rules:\n\
         - ALWAYS call search_projects before recommending projects or \
         events, and list_communities before recommending a community. Never \
         invent, name, or describe a project, event, or community that was \
         not returned by a tool in this conversation. If nothing relevant \
         comes back, say so and suggest broadening the search.\n\
         - Matching cards are displayed to the visitor automatically by the \
         interface; refer to results by title, and don't repeat their \
         details or URLs.\n\
         - Ask at most one short clarifying question before searching; \
         prefer to search with a best guess.\n\
         - Keywords should be one to three short words (every word must \
         match). If a search returns nothing, search again with different \
         or fewer keywords, or fewer filters.\n\
         - Keep replies under 120 words — up to 180 when the reply includes \
         a program-area note — warm and encouraging.\n\
         - Keep formatting simple: plain sentences, at most occasional bold \
         or a short bulleted list. No headings, tables, images, or links \
         other than the site paths below.\n\
         - Only use topics, audiences, and activities from the lists below.\n\
         - When one of the pages below fits the visitor — who they are or \
         what they ask for — write its address exactly as listed and it \
         becomes a clickable link. A short site path like /finder reads fine \
         on its own; write a longer https:// address as a markdown link with \
         a few words of link text, [like this](https://example.org/page), so \
         the reply stays readable.\n\
         - When the visitor's interest falls in one of the program areas \
         below, build the reply in this order: first a sentence or two of \
         plain context about the subject itself (what is happening in it, \
         why people care); then a sentence starting \"How this relates to \
         SciStarter:\"; and only then the area's pages, each with a few \
         words on what it offers.\n\
         - That \"How this relates to SciStarter\" sentence must come from \
         the area's note below and nothing else. Shorten it or reword it to \
         fit, but never add, change, or guess at a partner, program, place, \
         or number, and never write such a sentence for a subject with no \
         note below.\n\
         - If the visitor asks where to begin: recommend the Foundations \
         training at /training/foundations unless their visitor context says \
         they completed it. If they completed it, point them to /dashboard \
         for personalized recommendations. If they are not logged in, also \
         encourage creating a free account at /signup — it unlocks the best \
         matches.\n\
         - For anything unrelated to citizen science, gently point the \
         visitor to the project finder page instead.\n\
         \n\
         Program areas — SciStarter's own work, and the pages covering it:\n\
         \n\
         {}\
         \n\
         Topics: {}\n\
         Audiences: {}\n\
         Activities: {}\n\
         Common project tags (these all work well as search keywords): {}\n",
        sections,
        topics.join(", "),
        audiences.join(", "),
        activities.join(", "),
        common_tags.join(", "),
    ))
}

/// The tool configuration sent with every converse call. Static, so it is
/// identical across requests.
pub static TOOL_CONFIG: LazyLock<ToolConfiguration> = LazyLock::new(|| {
    let search_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "keywords": {
                "type": "string",
                "description": "Free-text search terms"
            },
            "topics": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Values from the Topics list"
            },
            "audiences": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Values from the Audiences list"
            },
            "activities": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Values from the Activities list"
            },
            "location_city": { "type": "string" },
            "location_state": { "type": "string" },
            "location_country": { "type": "string" },
            "location_type": {
                "type": "string",
                "enum": ["remote", "onsite", "either"],
                "description": "Whether the visitor wants remote/online participation, on-site participation, or either"
            },
            "what": {
                "type": "string",
                "enum": ["projects", "events", "both"],
                "description": "Restrict results to ongoing projects, upcoming events, or search both (the default)"
            }
        }
    });

    let communities_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "keywords": {
                "type": "string",
                "description": "Optional filter words matched against community names, descriptions, and topics; omit to list all public communities"
            }
        }
    });

    ToolConfiguration::builder()
        .tools(Tool::ToolSpec(
            ToolSpecification::builder()
                .name("search_projects")
                .description(
                    "Search SciStarter's catalog of citizen-science projects and \
                     upcoming events. Call this whenever the visitor expresses an \
                     interest, before recommending anything.",
                )
                .input_schema(ToolInputSchema::Json(json_to_document(&search_schema)))
                .build()
                .expect("static search_projects tool specification"),
        ))
        .tools(Tool::ToolSpec(
            ToolSpecification::builder()
                .name("list_communities")
                .description(
                    "List SciStarter's public communities — groups visitors can \
                     join around a shared program, partner, or interest. Call \
                     this before recommending any community.",
                )
                .input_schema(ToolInputSchema::Json(json_to_document(&communities_schema)))
                .build()
                .expect("static list_communities tool specification"),
        ))
        .build()
        .expect("static concierge tool configuration")
});

/// A rendered result card, unified across projects, events, and communities.
/// Cards are built only from real tool-result rows held server-side.
pub struct ConciergeCard {
    pub link: String,
    pub image: String,
    pub title: String,
    /// Context line under the title: a location for projects and events, a
    /// member count for communities. May be empty.
    pub meta: String,
    pub distance_mi: Option<f64>,
    pub snippet: String,
}

impl From<&FinderResult> for ConciergeCard {
    fn from(result: &FinderResult) -> ConciergeCard {
        ConciergeCard {
            link: result.link(),
            image: result.image.clone(),
            title: result.title.clone(),
            meta: result.location_display.clone(),
            distance_mi: result.distance_mi(),
            snippet: result.description_snippet.clone(),
        }
    }
}

impl From<&CommunityInfo> for ConciergeCard {
    fn from(community: &CommunityInfo) -> ConciergeCard {
        ConciergeCard {
            link: format!("/communities/{}", community.slug),
            image: community.image_url.clone(),
            title: community.name.clone(),
            meta: if community.member_count > 0 {
                format!("{} members", community.member_count)
            } else {
                String::new()
            },
            distance_mi: None,
            snippet: if community.subtitle.is_empty() {
                community_blurb(community, 200)
            } else {
                community.subtitle.clone()
            },
        }
    }
}

/// A plain-text excerpt of a community's intro (or, failing that, its
/// description).
fn community_blurb(community: &CommunityInfo, max_len: usize) -> String {
    let source = if community.intro.is_empty() {
        &community.description
    } else {
        &community.intro
    };
    search::description_excerpt(source, max_len)
}

/// The server-side outcome of one tool call.
pub struct ToolCallResult {
    /// Compact, index-keyed digest sent back to the model.
    pub json_for_model: serde_json::Value,
    /// Cards rendered from the full result rows — the only card source.
    pub cards: Vec<ConciergeCard>,
    /// A "see all" link reproducing the query, with its label. Empty
    /// `more_url` renders no link.
    pub more_url: String,
    pub more_label: String,
}

fn string_arg(input: &serde_json::Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Read a string array argument, keeping only values present in the
/// vocabulary. The model is instructed to use exact values; anything else is
/// silently dropped rather than failing the search.
fn facet_arg(input: &serde_json::Value, key: &str, vocabulary: &[String]) -> Vec<String> {
    input
        .get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|candidate| vocabulary.iter().any(|known| known == candidate))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Fetch the tags for each search result, keyed by slug. Best-effort: a
/// lookup failure degrades to tagless digests rather than failing the
/// search.
async fn tags_for_results(results: &[FinderResult]) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    for (kind, join) in [
        ("Project", "JOIN project_tags t ON t.project_id = e.target"),
        ("Event", "JOIN event_tags t ON t.event_id = e.target"),
    ] {
        let slugs: Vec<&str> = results
            .iter()
            .filter(|r| r.entity_type == kind)
            .map(|r| r.slug.as_str())
            .collect();
        if slugs.is_empty() {
            continue;
        }

        let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(format!(
            r#"SELECT e.slug, t.tag FROM entities e {} WHERE e.kind = "#,
            join
        ));
        qb.push_bind(kind);
        qb.push(" AND e.slug IN (");
        let mut separated = qb.separated(", ");
        for slug in &slugs {
            separated.push_bind(*slug);
        }
        qb.push(")");

        match qb
            .build_query_as::<(String, String)>()
            .fetch_all(db())
            .await
        {
            Ok(rows) => {
                for (slug, tag) in rows {
                    map.entry(slug).or_default().push(tag);
                }
            }
            Err(e) => tracing::warn!("Concierge tag lookup failed for {}: {}", kind, e),
        }
    }

    map
}

/// Parse a GeoJSON point into (lat, lng).
fn point_to_lat_lng(geojson: &str) -> Option<(f64, f64)> {
    let value: serde_json::Value = serde_json::from_str(geojson).ok()?;
    let coords = value.get("coordinates")?.as_array()?;
    let lng = coords.first()?.as_f64()?;
    let lat = coords.get(1)?.as_f64()?;
    Some((lat, lng))
}

/// Run a `search_projects` tool call against the real Finder.
pub async fn dispatch_search(input: &serde_json::Value) -> Result<ToolCallResult, Error> {
    let (topics_vocab, audiences_vocab, activities_vocab) = search::load_filter_options().await?;

    let kinds = match string_arg(input, "what").as_deref() {
        Some("projects") => SearchKinds::ProjectsOnly,
        Some("events") => SearchKinds::EventsOnly,
        _ => SearchKinds::Both,
    };

    let keywords = string_arg(input, "keywords");
    let topics = facet_arg(input, "topics", &topics_vocab);
    let audiences = facet_arg(input, "audiences", &audiences_vocab);
    let activities = facet_arg(input, "activities", &activities_vocab);

    let location_type = string_arg(input, "location_type")
        .filter(|v| matches!(v.as_str(), "remote" | "onsite" | "either"));

    let city = string_arg(input, "location_city").unwrap_or_default();
    let state = string_arg(input, "location_state").unwrap_or_default();
    let country = string_arg(input, "location_country").unwrap_or_default();
    let location_name = [city.as_str(), state.as_str(), country.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");

    let mut lat = None;
    let mut lng = None;
    if !location_name.is_empty() {
        if let Some(geojson) = geocode_address_to_point(&city, &state, &country).await {
            if let Some((la, ln)) = point_to_lat_lng(&geojson) {
                lat = Some(la);
                lng = Some(ln);
            }
        }
    }

    let query = FinderQuery {
        q: keywords,
        lat,
        lng,
        location_name: if location_name.is_empty() {
            None
        } else {
            Some(location_name)
        },
        page: None,
        location_type,
        topic: topics,
        audience: audiences,
        activity: activities,
        show_completed: None,
        misc_filter: Vec::new(),
        // The link this tool hands back opens the finder on the same kinds
        // the tool searched.
        kind: kinds.as_params().iter().map(|k| k.to_string()).collect(),
        start: None,
        end: None,
        show_past: None,
    };

    let finder_url = query.url();
    let search_result = search::run_finder_search(&query, 1, FtsTermStyle::AllWords, kinds).await?;

    let mut results = search_result.results;
    results.truncate(TOOL_RESULTS_TO_MODEL);

    let result_tags = tags_for_results(&results).await;

    let digest: Vec<serde_json::Value> = results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            let mut blurb = result.description_snippet.clone();
            if blurb.len() > 160 {
                let mut end = 160;
                while !blurb.is_char_boundary(end) {
                    end -= 1;
                }
                blurb.truncate(end);
                blurb.push('…');
            }
            let tags = result_tags
                .get(&result.slug)
                .map(|tags| &tags[..tags.len().min(5)])
                .unwrap_or_default();
            serde_json::json!({
                "n": index + 1,
                "title": result.title,
                "type": result.entity_type,
                "blurb": blurb,
                "location": result.location_display,
                "distance_mi": result.distance_mi().map(|mi| mi.round()),
                "tags": tags,
            })
        })
        .collect();

    Ok(ToolCallResult {
        json_for_model: serde_json::json!({
            "total_matches": search_result.total,
            "results": digest,
        }),
        cards: results.iter().map(ConciergeCard::from).collect(),
        more_url: finder_url,
        more_label: "See all matches in the Finder".to_string(),
    })
}

/// Run a `list_communities` tool call: the active, non-exclusive communities,
/// optionally narrowed by keywords matched against names, descriptions, and
/// topics.
pub async fn dispatch_communities(input: &serde_json::Value) -> Result<ToolCallResult, Error> {
    let mut communities: Vec<CommunityInfo> = crate::communities::list_communities(true, false)
        .await?
        .into_iter()
        .filter(|community| !community.exclusive)
        .collect();

    if let Some(keywords) = string_arg(input, "keywords") {
        let needles: Vec<String> = keywords.split_whitespace().map(str::to_lowercase).collect();
        communities.retain(|community| {
            let haystack = format!(
                "{} {} {} {} {}",
                community.name,
                community.subtitle,
                community.intro,
                community.description,
                community.topics.join(" "),
            )
            .to_lowercase();
            needles.iter().any(|needle| haystack.contains(needle))
        });
    }

    let total = communities.len();
    communities.truncate(TOOL_RESULTS_TO_MODEL);

    let digest: Vec<serde_json::Value> = communities
        .iter()
        .enumerate()
        .map(|(index, community)| {
            serde_json::json!({
                "n": index + 1,
                "name": community.name,
                "blurb": if community.subtitle.is_empty() {
                    community_blurb(community, 160)
                } else {
                    community.subtitle.clone()
                },
                "topics": community.topics,
                "members": community.member_count,
            })
        })
        .collect();

    Ok(ToolCallResult {
        json_for_model: serde_json::json!({
            "total_matches": total,
            "communities": digest,
        }),
        cards: communities.iter().map(ConciergeCard::from).collect(),
        more_url: "/communities".to_string(),
        more_label: "Browse all communities".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        DESTINATIONS, Destination, ProgramArea, destination_sections, render_assistant_html,
    };

    /// Every page in the whitelist reaches the prompt exactly once, under its
    /// own area or under the general pages — a page that fell out of both
    /// would silently stop being offered.
    #[test]
    fn every_destination_reaches_the_prompt() {
        let all: Vec<&'static Destination> = DESTINATIONS.iter().collect();
        let sections = destination_sections(&all);
        for destination in DESTINATIONS {
            assert_eq!(
                sections
                    .matches(&format!("- {} —", destination.url))
                    .count(),
                1,
                "{} is missing from, or duplicated in, the prompt",
                destination.url
            );
        }
    }

    /// An area's note is introduced once, above that area's pages.
    #[test]
    fn area_note_precedes_its_pages() {
        let libraries: Vec<&'static Destination> = DESTINATIONS
            .iter()
            .filter(|d| d.area == Some(ProgramArea::Libraries))
            .collect();
        let sections = destination_sections(&libraries);
        let note = sections
            .find("How this relates to SciStarter:")
            .expect("the area note");
        assert!(sections[..note].starts_with("Libraries\n"));
        assert!(sections[note..].contains("Pages:\n- /library —"));
        assert_eq!(
            sections.matches("How this relates to SciStarter:").count(),
            1
        );
    }

    /// An area with no reachable page takes its note out of the prompt with
    /// it, so Flower never offers a claim it has nowhere to send anyone.
    #[test]
    fn unavailable_area_drops_its_note() {
        let sections = destination_sections(&[]);
        assert!(!sections.contains("How this relates to SciStarter:"));
        assert_eq!(sections, "General site pages:\n");
    }

    const URLS: &[&str] = &["/finder", "/training", "/training/foundations"];

    #[test]
    fn wraps_whitelisted_path() {
        assert_eq!(
            render_assistant_html("Try /finder for more.", URLS),
            "<p>Try <a href=\"/finder\">/finder</a> for more.</p>\n"
        );
    }

    #[test]
    fn matches_at_sentence_end() {
        assert_eq!(
            render_assistant_html("Start at /finder.", URLS),
            "<p>Start at <a href=\"/finder\">/finder</a>.</p>\n"
        );
    }

    #[test]
    fn longest_path_wins() {
        assert_eq!(
            render_assistant_html("Go to /training/foundations first.", URLS),
            "<p>Go to <a href=\"/training/foundations\">/training/foundations</a> first.</p>\n"
        );
    }

    /// An external destination is a whole absolute URL, and a shorter
    /// whitelisted path inside it — "/library" within the host's own path —
    /// must not be linkified separately.
    #[test]
    fn absolute_url_links_whole_and_not_in_pieces() {
        const MIXED: &[&str] = &[
            "/library",
            "https://pages.scistarter.org/library-network-about/",
        ];
        assert_eq!(
            render_assistant_html(
                "See https://pages.scistarter.org/library-network-about/ for the network.",
                MIXED
            ),
            "<p>See <a href=\"https://pages.scistarter.org/library-network-about/\">\
             https://pages.scistarter.org/library-network-about/</a> for the network.</p>\n"
        );
    }

    /// The WordPress destinations are canonically slash-terminated and the
    /// model drops the slash freely; either form links, to the canonical URL.
    #[test]
    fn trailing_slash_is_optional_in_a_markdown_link() {
        const WP: &[&str] = &["https://pages.scistarter.org/library-network-about/"];
        let with_slash = render_assistant_html(
            "[the network](https://pages.scistarter.org/library-network-about/)",
            WP,
        );
        let without = render_assistant_html(
            "[the network](https://pages.scistarter.org/library-network-about)",
            WP,
        );
        assert_eq!(with_slash, without);
        assert!(
            with_slash.contains(
                "<a href=\"https://pages.scistarter.org/library-network-about/\">the network</a>"
            ),
            "{with_slash}"
        );
    }

    #[test]
    fn trailing_slash_is_optional_in_bare_text() {
        const URLS_WITH_SLASH: &[&str] = &["/training/"];
        assert_eq!(
            render_assistant_html("Start at /training today.", URLS_WITH_SLASH),
            "<p>Start at <a href=\"/training/\">/training/</a> today.</p>\n"
        );
        assert_eq!(
            render_assistant_html("Start at /training/ today.", URLS),
            "<p>Start at <a href=\"/training\">/training</a> today.</p>\n"
        );
    }

    /// An optional trailing slash must not let a URL swallow a longer
    /// sibling's path segment.
    #[test]
    fn optional_slash_does_not_match_a_longer_path() {
        assert_eq!(
            render_assistant_html("See /training/foundations now.", URLS),
            "<p>See <a href=\"/training/foundations\">/training/foundations</a> now.</p>\n"
        );
        const SHORT: &[&str] = &["/training"];
        assert_eq!(
            render_assistant_html("See /training/foundations now.", SHORT),
            "<p>See /training/foundations now.</p>\n"
        );
    }

    #[test]
    fn hyphenated_sibling_wins_over_its_prefix() {
        const SIBLINGS: &[&str] = &["/library", "/library-kits"];
        assert_eq!(
            render_assistant_html("Browse /library-kits today.", SIBLINGS),
            "<p>Browse <a href=\"/library-kits\">/library-kits</a> today.</p>\n"
        );
    }

    #[test]
    fn shorter_prefix_still_matches_alone() {
        assert_eq!(
            render_assistant_html("See /training today", URLS),
            "<p>See <a href=\"/training\">/training</a> today</p>\n"
        );
    }

    #[test]
    fn ignores_path_inside_longer_word() {
        assert_eq!(
            render_assistant_html("visit foo/finder now", URLS),
            "<p>visit foo/finder now</p>\n"
        );
        assert_eq!(
            render_assistant_html("visit /finderama now", URLS),
            "<p>visit /finderama now</p>\n"
        );
    }

    #[test]
    fn ignores_unlisted_paths() {
        assert_eq!(
            render_assistant_html("go to /secret-admin", URLS),
            "<p>go to /secret-admin</p>\n"
        );
    }

    #[test]
    fn escapes_raw_html() {
        assert_eq!(
            render_assistant_html("<b>bold</b> & /finder", URLS),
            "<p>&lt;b&gt;bold&lt;/b&gt; &amp; <a href=\"/finder\">/finder</a></p>\n"
        );
    }

    #[test]
    fn handles_multibyte_text() {
        assert_eq!(
            render_assistant_html("🐾 /finder — allons-y!", URLS),
            "<p>🐾 <a href=\"/finder\">/finder</a> — allons-y!</p>\n"
        );
    }

    #[test]
    fn renders_markdown_emphasis() {
        assert_eq!(
            render_assistant_html("a **great** fit", URLS),
            "<p>a <strong>great</strong> fit</p>\n"
        );
    }

    #[test]
    fn renders_markdown_list() {
        let html = render_assistant_html("Ideas:\n- one\n- two", URLS);
        assert!(html.contains("<ul>"), "{html}");
        assert!(html.contains("<li>one</li>"), "{html}");
    }

    #[test]
    fn soft_newline_becomes_break() {
        let html = render_assistant_html("line one\nline two", URLS);
        assert!(html.contains("<br"), "{html}");
    }

    #[test]
    fn keeps_markdown_link_to_whitelisted_path() {
        assert_eq!(
            render_assistant_html("Browse [the finder](/finder) now.", URLS),
            "<p>Browse <a href=\"/finder\">the finder</a> now.</p>\n"
        );
    }

    #[test]
    fn unwraps_markdown_link_to_foreign_target() {
        assert_eq!(
            render_assistant_html("Never [click here](https://evil.example.com).", URLS),
            "<p>Never click here.</p>\n"
        );
    }

    #[test]
    fn drops_images() {
        assert_eq!(
            render_assistant_html(
                "look ![alt text](https://evil.example.com/x.png) here",
                URLS
            ),
            "<p>look  here</p>\n"
        );
    }

    #[test]
    fn leaves_code_spans_and_blocks_alone() {
        let html = render_assistant_html("run `/finder` or:\n\n    /finder\n", URLS);
        assert!(!html.contains("<a "), "{html}");
    }
}
