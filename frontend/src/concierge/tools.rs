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
//! of destination paths, and [`render_assistant_html`] turns exactly those
//! paths — nothing else — into anchors while interpreting the model's light
//! markdown with raw HTML escaped and foreign link targets stripped.

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
    /// prompt until their content arrives, so Flower never offers a path
    /// that would 404.
    Entity(&'static str),
}

/// A site page Flower may direct visitors to.
struct Destination {
    path: &'static str,
    purpose: &'static str,
    source: DestinationSource,
}

/// The curated navigation whitelist. These are the only paths that render as
/// links in assistant replies.
const DESTINATIONS: &[Destination] = &[
    Destination {
        path: "/training/foundations",
        purpose: "the Foundations of Citizen Science training — the best \
                  first step for anyone new to citizen science",
        source: DestinationSource::Native,
    },
    Destination {
        path: "/training",
        purpose: "free trainings that earn badges and certificates",
        source: DestinationSource::Entity("training"),
    },
    Destination {
        path: "/dashboard",
        purpose: "personalized project recommendations for logged-in visitors",
        source: DestinationSource::Native,
    },
    Destination {
        path: "/signup",
        purpose: "creating a free SciStarter account",
        source: DestinationSource::Native,
    },
    Destination {
        path: "/education",
        purpose: "students and teachers",
        source: DestinationSource::Entity("education"),
    },
    Destination {
        path: "/library",
        purpose: "librarians and library staff",
        source: DestinationSource::Entity("library"),
    },
    Destination {
        path: "/nasa",
        purpose: "NASA fans and space enthusiasts",
        source: DestinationSource::Entity("nasa"),
    },
    Destination {
        path: "/girlscouts-info",
        purpose: "Girl Scouts and their leaders",
        source: DestinationSource::Entity("girlscouts-info"),
    },
    Destination {
        path: "/ambassadors",
        purpose: "the SciStarter Ambassador program",
        source: DestinationSource::Native,
    },
    Destination {
        path: "/communities",
        purpose: "browsing all public communities",
        source: DestinationSource::Native,
    },
    Destination {
        path: "/finder",
        purpose: "the full project finder",
        source: DestinationSource::Native,
    },
];

/// The destinations currently reachable on this site: native routes plus any
/// entity-backed pages whose content has been published. Degrades to native
/// routes only if the entity lookup fails.
async fn available_destinations() -> Vec<&'static Destination> {
    let slugs: Vec<&'static str> = DESTINATIONS
        .iter()
        .filter_map(|d| match d.source {
            DestinationSource::Entity(slug) => Some(slug),
            DestinationSource::Native => None,
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
            DestinationSource::Native => true,
            DestinationSource::Entity(slug) => published.iter().any(|p| p == slug),
        })
        .collect()
}

/// The paths eligible for linkification in assistant replies.
pub async fn linkable_paths() -> Vec<&'static str> {
    available_destinations()
        .await
        .iter()
        .map(|d| d.path)
        .collect()
}

/// A piece of assistant text after path matching: literal text, or a
/// whitelisted destination path.
enum Segment {
    Text(String),
    Path(&'static str),
}

/// Split `text` on occurrences of the whitelisted `paths`. A path match must
/// stand alone: the characters on either side may not be word- or path-like,
/// so "/training" never matches inside "/training/foundations" or
/// "foo/training". Longest path wins at any position.
fn split_paths(text: &str, paths: &[&'static str]) -> Vec<Segment> {
    fn word_like(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'/')
    }

    let mut ordered: Vec<&'static str> = paths.to_vec();
    ordered.sort_by_key(|path| std::cmp::Reverse(path.len()));

    // Paths are ASCII, so byte offsets used for matching always land on
    // character boundaries.
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let preceded_by_word = at > 0 && word_like(text.as_bytes()[at - 1]);
        let hit = ordered.iter().find(|path| {
            !preceded_by_word
                && rest.starts_with(**path)
                && !matches!(rest.as_bytes().get(path.len()), Some(&next) if word_like(next))
        });
        match hit {
            Some(path) => {
                if !literal.is_empty() {
                    segments.push(Segment::Text(std::mem::take(&mut literal)));
                }
                segments.push(Segment::Path(path));
                at += path.len();
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

/// The anchor markup for a whitelisted destination path.
fn path_anchor(path: &str) -> String {
    format!(r#"<a href="{path}">{path}</a>"#)
}

/// Render assistant output as sanitized HTML. Haiku emits light markdown
/// despite prompting, so the text is interpreted as markdown rather than
/// shown with literal `**` markers — but on the model's-output-is-untrusted
/// terms:
///
/// - raw HTML becomes escaped text (`push_html` escapes `Text` events);
/// - markdown links are honored only when the destination is exactly a
///   whitelisted path — any other link is unwrapped to its inner text, and
///   images are dropped;
/// - bare whitelisted paths in ordinary text become anchors;
/// - soft line breaks render as real breaks, chat-style.
///
/// This remains the only route from model output to markup, and it can only
/// produce links to the curated destinations.
pub fn render_assistant_html(text: &str, paths: &[&'static str]) -> String {
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
                let keep = paths.contains(&dest_url.as_ref());
                if keep {
                    events.push(Event::Start(Tag::Link {
                        link_type: LinkType::Inline,
                        dest_url,
                        title: "".into(),
                        id: "".into(),
                    }));
                }
                link_kept.push(keep);
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
            // Linkify bare paths in ordinary text — not inside an existing
            // link or a code block.
            Event::Text(text) if link_kept.is_empty() && code_depth == 0 => {
                for segment in split_paths(&text, paths) {
                    match segment {
                        Segment::Text(literal) => events.push(Event::Text(literal.into())),
                        Segment::Path(path) => events.push(Event::Html(path_anchor(path).into())),
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
    let destinations = available_destinations().await;

    let destination_lines: String = destinations
        .iter()
        .map(|d| format!("- {} — {}\n", d.path, d.purpose))
        .collect();

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
         - Keep replies under 120 words, warm and encouraging.\n\
         - Keep formatting simple: plain sentences, at most occasional bold \
         or a short bulleted list. No headings, tables, images, or links \
         other than the site paths below.\n\
         - Only use topics, audiences, and activities from the lists below.\n\
         - When one of the site pages below fits the visitor — who they are \
         or what they ask for — mention its path, exactly as written, and it \
         will be shown as a clickable link.\n\
         - If the visitor asks where to begin: recommend the Foundations \
         training at /training/foundations unless their visitor context says \
         they completed it. If they completed it, point them to /dashboard \
         for personalized recommendations. If they are not logged in, also \
         encourage creating a free account at /signup — it unlocks the best \
         matches.\n\
         - For anything unrelated to citizen science, gently point the \
         visitor to the project finder page instead.\n\
         \n\
         Site pages:\n\
         {}\
         \n\
         Topics: {}\n\
         Audiences: {}\n\
         Activities: {}\n\
         Common project tags (these all work well as search keywords): {}\n",
        destination_lines,
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
    };

    let finder_url = finder_url_for(&query);
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

/// Build a `/finder` URL reproducing this query.
fn finder_url_for(query: &FinderQuery) -> String {
    let mut params = Vec::new();
    if let Some(q) = &query.q {
        params.push(format!("q={}", urlencoding::encode(q)));
    }
    if let (Some(lat), Some(lng)) = (query.lat, query.lng) {
        params.push(format!("lat={}", lat));
        params.push(format!("lng={}", lng));
    }
    if let Some(name) = &query.location_name {
        params.push(format!("location_name={}", urlencoding::encode(name)));
    }
    if let Some(location_type) = &query.location_type {
        if location_type != "either" {
            params.push(format!("location_type={}", location_type));
        }
    }
    for topic in &query.topic {
        params.push(format!("topic[]={}", urlencoding::encode(topic)));
    }
    for audience in &query.audience {
        params.push(format!("audience[]={}", urlencoding::encode(audience)));
    }
    for activity in &query.activity {
        params.push(format!("activity[]={}", urlencoding::encode(activity)));
    }

    if params.is_empty() {
        "/finder".to_string()
    } else {
        format!("/finder?{}", params.join("&"))
    }
}

#[cfg(test)]
mod tests {
    use super::render_assistant_html;

    const PATHS: &[&str] = &["/finder", "/training", "/training/foundations"];

    #[test]
    fn wraps_whitelisted_path() {
        assert_eq!(
            render_assistant_html("Try /finder for more.", PATHS),
            "<p>Try <a href=\"/finder\">/finder</a> for more.</p>\n"
        );
    }

    #[test]
    fn matches_at_sentence_end() {
        assert_eq!(
            render_assistant_html("Start at /finder.", PATHS),
            "<p>Start at <a href=\"/finder\">/finder</a>.</p>\n"
        );
    }

    #[test]
    fn longest_path_wins() {
        assert_eq!(
            render_assistant_html("Go to /training/foundations first.", PATHS),
            "<p>Go to <a href=\"/training/foundations\">/training/foundations</a> first.</p>\n"
        );
    }

    #[test]
    fn shorter_prefix_still_matches_alone() {
        assert_eq!(
            render_assistant_html("See /training today", PATHS),
            "<p>See <a href=\"/training\">/training</a> today</p>\n"
        );
    }

    #[test]
    fn ignores_path_inside_longer_word() {
        assert_eq!(
            render_assistant_html("visit foo/finder now", PATHS),
            "<p>visit foo/finder now</p>\n"
        );
        assert_eq!(
            render_assistant_html("visit /finderama now", PATHS),
            "<p>visit /finderama now</p>\n"
        );
    }

    #[test]
    fn ignores_unlisted_paths() {
        assert_eq!(
            render_assistant_html("go to /secret-admin", PATHS),
            "<p>go to /secret-admin</p>\n"
        );
    }

    #[test]
    fn escapes_raw_html() {
        assert_eq!(
            render_assistant_html("<b>bold</b> & /finder", PATHS),
            "<p>&lt;b&gt;bold&lt;/b&gt; &amp; <a href=\"/finder\">/finder</a></p>\n"
        );
    }

    #[test]
    fn handles_multibyte_text() {
        assert_eq!(
            render_assistant_html("🐾 /finder — allons-y!", PATHS),
            "<p>🐾 <a href=\"/finder\">/finder</a> — allons-y!</p>\n"
        );
    }

    #[test]
    fn renders_markdown_emphasis() {
        assert_eq!(
            render_assistant_html("a **great** fit", PATHS),
            "<p>a <strong>great</strong> fit</p>\n"
        );
    }

    #[test]
    fn renders_markdown_list() {
        let html = render_assistant_html("Ideas:\n- one\n- two", PATHS);
        assert!(html.contains("<ul>"), "{html}");
        assert!(html.contains("<li>one</li>"), "{html}");
    }

    #[test]
    fn soft_newline_becomes_break() {
        let html = render_assistant_html("line one\nline two", PATHS);
        assert!(html.contains("<br"), "{html}");
    }

    #[test]
    fn keeps_markdown_link_to_whitelisted_path() {
        assert_eq!(
            render_assistant_html("Browse [the finder](/finder) now.", PATHS),
            "<p>Browse <a href=\"/finder\">the finder</a> now.</p>\n"
        );
    }

    #[test]
    fn unwraps_markdown_link_to_foreign_target() {
        assert_eq!(
            render_assistant_html("Never [click here](https://evil.example.com).", PATHS),
            "<p>Never click here.</p>\n"
        );
    }

    #[test]
    fn drops_images() {
        assert_eq!(
            render_assistant_html(
                "look ![alt text](https://evil.example.com/x.png) here",
                PATHS
            ),
            "<p>look  here</p>\n"
        );
    }

    #[test]
    fn leaves_code_spans_and_blocks_alone() {
        let html = render_assistant_html("run `/finder` or:\n\n    /finder\n", PATHS);
        assert!(!html.contains("<a "), "{html}");
    }
}
