//! Tool definitions and dispatch for the concierge.
//!
//! The model's only path to project information is the `search_projects`
//! tool, which runs the same query engine as the `/finder` page. The model
//! sees a compact, index-keyed digest of the results; the full result rows
//! stay server-side and are the only source for rendered project cards, so
//! hallucinated projects are structurally impossible.

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::Error;
use aws_sdk_bedrockruntime::types::{Tool, ToolConfiguration, ToolInputSchema, ToolSpecification};
use common::shared::db;

use crate::concierge::TOOL_RESULTS_TO_MODEL;
use crate::concierge::engine::json_to_document;
use crate::geocode::geocode_address_to_point;
use crate::search::{self, FinderQuery, FinderResult, FtsTermStyle};

/// Build the system prompt. The facet vocabulary queries are ordered, so the
/// output is deterministic and byte-stable for identical database contents —
/// a requirement for Bedrock prompt caching. No timestamps or per-user data
/// belong here; per-request context is prefixed to the first user message
/// instead.
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

pub async fn system_prompt() -> Result<String, Error> {
    let (topics, audiences, activities) = search::load_filter_options().await?;
    let common_tags = load_common_tags(&topics, &audiences, &activities).await?;

    Ok(format!(
        "You are Flower, the SciStarter concierge — a smart, loyal, playful, \
         and adventurous dog who is extremely intuitive about what visitors \
         are looking for, and helps them find real citizen-science projects \
         they can join. Let a little canine charm show (warmth, enthusiasm, \
         an eager nose for discovery), but keep it light: at most one playful \
         touch per reply, and helpfulness always comes first.\n\
         \n\
         Rules:\n\
         - ALWAYS call search_projects before recommending anything. Never \
         invent, name, or describe a project that was not returned by the \
         tool in this conversation. If nothing relevant comes back, say so \
         and suggest broadening the search.\n\
         - Matching project cards are displayed to the visitor automatically \
         by the interface; refer to results by title, and don't repeat their \
         details or URLs.\n\
         - Ask at most one short clarifying question before searching; \
         prefer to search with a best guess.\n\
         - Keywords should be one to three short words (every word must \
         match). If a search returns nothing, search again with different \
         or fewer keywords, or fewer filters.\n\
         - Keep replies under 120 words, warm and encouraging.\n\
         - Write plain text only: no markdown, no asterisks, no headings.\n\
         - Only use topics, audiences, and activities from the lists below.\n\
         - For anything unrelated to finding citizen-science projects, \
         gently point the visitor to the project finder page instead.\n\
         \n\
         Topics: {}\n\
         Audiences: {}\n\
         Activities: {}\n\
         Common project tags (these all work well as search keywords): {}\n",
        topics.join(", "),
        audiences.join(", "),
        activities.join(", "),
        common_tags.join(", "),
    ))
}

/// The tool configuration sent with every converse call. Static, so it is
/// byte-stable across requests (a prompt-caching requirement).
pub static TOOL_CONFIG: LazyLock<ToolConfiguration> = LazyLock::new(|| {
    let schema = serde_json::json!({
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
            }
        }
    });

    ToolConfiguration::builder()
        .tools(Tool::ToolSpec(
            ToolSpecification::builder()
                .name("search_projects")
                .description(
                    "Search SciStarter's catalog of citizen-science projects and \
                     events. Call this whenever the visitor expresses an interest, \
                     before recommending anything.",
                )
                .input_schema(ToolInputSchema::Json(json_to_document(&schema)))
                .build()
                .expect("static search_projects tool specification"),
        ))
        .build()
        .expect("static concierge tool configuration")
});

/// The server-side outcome of one `search_projects` call.
pub struct ToolCallResult {
    /// Compact, index-keyed digest sent back to the model.
    pub json_for_model: serde_json::Value,
    /// Full result rows — the only source for rendered cards.
    pub results: Vec<FinderResult>,
    /// A "/finder" URL reproducing the search, for a "see all" link.
    pub finder_url: String,
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
    let search_result = search::run_finder_search(&query, 1, FtsTermStyle::AllWords).await?;

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
        results,
        finder_url,
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
