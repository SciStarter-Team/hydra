//! The sitemap at `/sitemap.xml`, and the `/robots.txt` that points crawlers
//! to it.
//!
//! The sitemap is built from this instance's database whenever it is
//! fetched, so there is nothing to regenerate: a page is in it from the
//! moment it is published until the moment it is unpublished. Every frontend
//! instance has its own copy of the entities, kept in step by peering, so
//! whichever instance answers gives the same list.
//!
//! When a page last changed is the activity log's to say: it is the latest
//! [`PageChange`](common::message::log::PageChange) logged against what the page shows, which `log.last_changed`
//! reports and each URL carries as its `lastmod`. A page with no change
//! logged, or one that shows nothing the log follows, has no `lastmod`, which
//! is better than one that is wrong.
//!
//! `/sitemap.xml` is a sitemap index over one sitemap per [`Section`], which
//! keeps the kinds of page apart in a search console's coverage reports. A
//! section that outgrows the [`MAX_URLS`] the protocol allows one sitemap is
//! split into parts, each a sitemap of its own.
//!
//! What it lists is what a search engine should index: the fixed public
//! pages, published content pages, communities, the published pages of the
//! projects the finder lists, published events, and the profiles of the
//! ambassadors the directory lists. Left out are redirects, which are the way
//! to a page rather than a page; the pages of other people (profiles, badge
//! awards); lists, whose views are the statistics their owners are shown; and
//! the exchange and partner-embedded copies of project pages.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use anyhow::Error;
use common::message::user::{
    AmbassadorDirectoryListingRequest, AmbassadorDirectoryListingResponse,
};
use common::service::request;
use common::shared::db;
use common::timestring::TimeString;
use poem::{
    IntoResponse, Response, handler,
    http::StatusCode,
    web::{Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use sqlx::SqlitePool;
use ulid::Ulid;
use url::Url;

use crate::communities::list_communities;
use crate::entity::EntityKind;
use crate::page_change::last_changes;
use crate::project::{ProjectStatus, push_listing_rule};
use crate::training::{self, dataethics::DataEthicsLanguage};

const DOMAIN: &str = env!("DOMAIN");

/// The most URLs the sitemap protocol allows in one sitemap.
const MAX_URLS: usize = 50_000;

static SITE: LazyLock<Url> =
    LazyLock::new(|| Url::parse(&format!("https://{DOMAIN}/")).expect("DOMAIN is not a host name"));

/// The absolute URL of `path` on this site, percent-encoded as the protocol
/// requires.
fn absolute(path: &str) -> Url {
    let mut url = SITE.clone();
    url.set_path(path);
    url
}

/// Each path a section lists, in order, with the target whose changes are
/// changes to the page there, if the log follows one.
type Paths = BTreeMap<String, Option<Ulid>>;

/// One sitemap of the index, and the pages it lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// The fixed public pages, published content pages and communities.
    Pages,
    /// The published pages of the projects the finder lists.
    Projects,
    /// The published pages of events.
    Events,
    /// The profiles of the ambassadors the directory lists.
    Ambassadors,
}

impl Section {
    const ALL: [Section; 4] = [
        Section::Pages,
        Section::Projects,
        Section::Events,
        Section::Ambassadors,
    ];

    /// The name of this section's sitemap under `/sitemap/`.
    fn file_name(self) -> &'static str {
        match self {
            Section::Pages => "pages.xml",
            Section::Projects => "projects.xml",
            Section::Events => "events.xml",
            Section::Ambassadors => "ambassadors.xml",
        }
    }

    fn from_file_name(file_name: &str) -> Option<Section> {
        Section::ALL
            .into_iter()
            .find(|section| section.file_name() == file_name)
    }

    /// The URL of the sitemap holding part `part` of this section, counting
    /// from 1.
    fn sitemap_url(self, part: usize) -> Url {
        let mut url = absolute(&format!("/sitemap/{}", self.file_name()));
        if part > 1 {
            url.set_query(Some(&format!("part={part}")));
        }
        url
    }

    async fn paths(self) -> Result<Paths, Error> {
        match self {
            Section::Pages => pages().await,
            Section::Projects => projects(db()).await,
            Section::Events => events().await,
            Section::Ambassadors => ambassadors().await,
        }
    }
}

/// How many sitemaps it takes to list `count` URLs. A section with nothing
/// in it still has its one, empty, sitemap.
fn part_count(count: usize) -> usize {
    count.div_ceil(MAX_URLS).max(1)
}

/// The public pages served by routes of their own rather than by entities.
fn fixed_pages() -> [&'static str; 17] {
    [
        "/",
        "/finder",
        "/data",
        "/communities",
        "/citizensciencemonth",
        "/citizensciencemonth/participate",
        "/api",
        crate::ambassador::DIRECTORY_PATH,
        training::foundations::BASE_PATH,
        DataEthicsLanguage::English.base_path(),
        DataEthicsLanguage::Spanish.base_path(),
        DataEthicsLanguage::Portuguese.base_path(),
        training::dataliteracy::BASE_PATH,
        training::highered::BASE_PATH,
        training::libraries::BASE_PATH,
        training::k12::BASE_PATH,
        training::champion::BASE_PATH,
    ]
}

/// Whether a published entity of `kind` is listed among the content pages.
fn is_content_page(kind: EntityKind) -> bool {
    match kind {
        EntityKind::RawHtml
        | EntityKind::DressedHtml
        | EntityKind::WordPress
        | EntityKind::Wysiwyg
        | EntityKind::PageBuilder => true,
        // Each has a section of its own.
        EntityKind::Project | EntityKind::Event => false,
        // A redirect is the way to a page rather than a page, a placeholder
        // has nothing on it yet, and a form page is not served yet.
        EntityKind::Redirect | EntityKind::Placeholder | EntityKind::Form => false,
    }
}

/// The paths of [`Section::Pages`]. A content page changes with its content,
/// and a community's page with the community.
async fn pages() -> Result<Paths, Error> {
    let mut paths: Paths = fixed_pages()
        .into_iter()
        .map(|path| (path.to_string(), None))
        .collect();

    let entities = sqlx::query!(
        r#"SELECT "slug", "kind" as "kind: EntityKind", "target"
           FROM "entities"
           WHERE "publish" = 1"#
    )
    .fetch_all(db())
    .await?;

    paths.extend(
        entities
            .into_iter()
            .filter(|entity| is_content_page(entity.kind))
            .map(|entity| (entity.kind.path(&entity.slug), entity.target.parse().ok())),
    );

    // The communities the landing page lists.
    paths.extend(
        list_communities(true, false)
            .await?
            .into_iter()
            .map(|community| {
                (
                    format!("/communities/{}", community.slug),
                    Some(community.community_id),
                )
            }),
    );

    Ok(paths)
}

/// The paths of [`Section::Projects`]: the published pages of every project
/// the finder lists when it is asked for finished projects too, which is
/// every project that may be listed at all.
async fn projects(pool: &SqlitePool) -> Result<Paths, Error> {
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT e.slug, p.project_id FROM entities e JOIN projects p ON p.project_id = e.target \
         WHERE e.kind = ",
    );
    qb.push_bind(EntityKind::Project);
    qb.push(" AND e.publish = 1");
    push_listing_rule(&mut qb, &ProjectStatus::LISTED_WHEN_INACTIVE);

    let rows = qb
        .build_query_as::<(String, String)>()
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(slug, project_id)| (EntityKind::Project.path(&slug), project_id.parse().ok()))
        .collect())
}

/// The paths of [`Section::Events`]. Past events are listed along with the
/// rest, as the pages of finished projects are.
async fn events() -> Result<Paths, Error> {
    let rows = sqlx::query!(
        r#"SELECT "slug", "target" FROM "entities" WHERE "kind" = ? AND "publish" = 1"#,
        EntityKind::Event,
    )
    .fetch_all(db())
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| (EntityKind::Event.path(&row.slug), row.target.parse().ok()))
        .collect())
}

/// The paths of [`Section::Ambassadors`]. What a profile shows comes from
/// the ambassador's account, application and connected events, none of which
/// logs its changes, so a profile carries no `lastmod`.
async fn ambassadors() -> Result<Paths, Error> {
    let listing: AmbassadorDirectoryListingResponse = request(
        "user.ambassador.directory_listing",
        AmbassadorDirectoryListingRequest {},
    )
    .await?;

    Ok(listing
        .user_ids
        .into_iter()
        .map(|user_id| (crate::ambassador::profile_path(user_id), None))
        .collect())
}

#[derive(TemplateOnce)]
#[template(path = "sitemap/index.stpl.xml")]
struct SitemapIndex {
    sitemaps: Vec<Url>,
}

/// One URL of a sitemap.
struct Entry {
    loc: Url,
    lastmod: Option<TimeString>,
}

#[derive(TemplateOnce)]
#[template(path = "sitemap/urlset.stpl.xml")]
struct UrlSet {
    entries: Vec<Entry>,
}

fn xml(body: String) -> Response {
    Response::builder()
        .content_type("application/xml; charset=utf-8")
        .body(body)
}

/// `/sitemap.xml`: the index of every section's sitemaps.
#[handler]
pub async fn sitemap_index() -> Result<Response, Error> {
    let mut sitemaps = Vec::new();
    for section in Section::ALL {
        let count = section.paths().await?.len();
        sitemaps.extend((1..=part_count(count)).map(|part| section.sitemap_url(part)));
    }

    Ok(xml(SitemapIndex { sitemaps }.render_once()?))
}

#[derive(Debug, Deserialize)]
pub struct PartQuery {
    part: Option<usize>,
}

/// `/sitemap/<section>.xml?part=<n>`: one part of a section's sitemap, the
/// first when no part is named.
#[handler]
pub async fn section_sitemap(
    Path(file_name): Path<String>,
    Query(query): Query<PartQuery>,
) -> Result<Response, Error> {
    let Some(section) = Section::from_file_name(&file_name) else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let paths = section.paths().await?;
    let part = query.part.unwrap_or(1);
    if part == 0 || part > part_count(paths.len()) {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }

    let listed: Vec<(&String, Option<Ulid>)> = paths
        .iter()
        .skip((part - 1) * MAX_URLS)
        .take(MAX_URLS)
        .map(|(path, target)| (path, *target))
        .collect();

    let changes = last_changes(listed.iter().filter_map(|(_, target)| *target).collect()).await?;

    let entries = listed
        .into_iter()
        .map(|(path, target)| Entry {
            loc: absolute(path),
            lastmod: target.and_then(|target| changes.get(&target).cloned()),
        })
        .collect();

    Ok(xml(UrlSet { entries }.render_once()?))
}

/// `/robots.txt`, which says where the sitemap is.
///
/// What crawlers may fetch is left to Cloudflare's managed robots.txt, which
/// it prepends to whatever the origin serves, so this says nothing else. A
/// `Sitemap` line belongs to no group of rules, which is what lets it follow
/// theirs.
#[handler]
pub async fn robots_txt() -> Response {
    Response::builder()
        .content_type("text/plain; charset=utf-8")
        .body(robots())
}

fn robots() -> String {
    format!("Sitemap: {}\n", absolute("/sitemap.xml"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use assert2::check;
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    #[test]
    fn a_section_is_split_only_once_it_outgrows_one_sitemap() {
        check!(part_count(0) == 1);
        check!(part_count(MAX_URLS) == 1);
        check!(part_count(MAX_URLS + 1) == 2);
        check!(part_count(2 * MAX_URLS + 1) == 3);
    }

    #[test]
    fn the_first_part_of_a_section_is_addressed_without_a_query() {
        let first = Section::Projects.sitemap_url(1);
        check!(first.path() == "/sitemap/projects.xml");
        check!(first.query() == None);

        let second = Section::Projects.sitemap_url(2);
        check!(second.path() == "/sitemap/projects.xml");
        check!(second.query() == Some("part=2"));
    }

    #[test]
    fn every_section_is_found_by_its_file_name() {
        for section in Section::ALL {
            check!(Section::from_file_name(section.file_name()) == Some(section));
        }
        check!(Section::from_file_name("sitemap.xml") == None);
    }

    #[test]
    fn paths_are_percent_encoded() {
        check!(absolute("/a b/é").path() == "/a%20b/%C3%A9");
        check!(absolute("/training/faq").path() == "/training/faq");
    }

    #[test]
    fn urls_are_escaped_for_xml() {
        let body = UrlSet {
            entries: vec![Entry {
                loc: absolute("/this&that"),
                lastmod: None,
            }],
        }
        .render_once()
        .unwrap();

        check!(body.starts_with("<?xml"));
        check!(body.contains("/this&amp;that</loc>"));
        check!(!body.contains("<lastmod>"));
    }

    #[test]
    fn a_known_change_is_the_lastmod() {
        let when = TimeString::try_from("2026-10-22T11:02:10Z".to_string()).unwrap();
        let body = UrlSet {
            entries: vec![Entry {
                loc: absolute("/projects/active"),
                lastmod: Some(when),
            }],
        }
        .render_once()
        .unwrap();

        check!(body.contains("<lastmod>2026-10-22T11:02:10Z</lastmod>"));
    }

    #[test]
    fn robots_txt_names_only_the_sitemap() {
        check!(robots() == format!("Sitemap: https://{DOMAIN}/sitemap.xml\n"));
    }

    /// One project per case the listing rule decides, named for it, each with
    /// a page published unless its name says otherwise, and the id each was
    /// given.
    async fn project_pages() -> (SqlitePool, HashMap<&'static str, Ulid>) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        sqlx::query("CREATE TABLE entities (slug TEXT, kind TEXT, target TEXT, publish BOOLEAN)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE projects (project_id TEXT, flagged INTEGER, duplicate INTEGER, \
             is_test INTEGER, draft INTEGER, status TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut ids = HashMap::new();
        for (name, status, flagged, duplicate, is_test, draft, publish) in [
            ("active", "Active", 0, 0, 0, 0, 1),
            ("pending", "Pending", 0, 0, 0, 0, 1),
            ("complete", "Complete", 0, 0, 0, 0, 1),
            ("expired", "Expired", 0, 0, 0, 0, 1),
            ("stale-draft", "Active", 0, 0, 0, 1, 1),
            ("hidden", "Hidden", 0, 0, 0, 0, 1),
            ("flagged", "Active", 1, 0, 0, 0, 1),
            ("duplicate", "Active", 0, 1, 0, 0, 1),
            ("test", "Active", 0, 0, 1, 0, 1),
            ("unpublished", "Active", 0, 0, 0, 0, 0),
        ] {
            let id = Ulid::new();
            ids.insert(name, id);
            sqlx::query("INSERT INTO projects VALUES (?, ?, ?, ?, ?, ?)")
                .bind(id.to_string())
                .bind(flagged)
                .bind(duplicate)
                .bind(is_test)
                .bind(draft)
                .bind(status)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO entities VALUES (?, ?, ?, ?)")
                .bind(name)
                .bind(EntityKind::Project)
                .bind(id.to_string())
                .bind(publish)
                .execute(&pool)
                .await
                .unwrap();
        }

        (pool, ids)
    }

    #[tokio::test]
    async fn the_sitemap_lists_the_published_projects_the_finder_can_list() {
        let (pool, ids) = project_pages().await;

        let listed = ["active", "complete", "expired", "pending", "stale-draft"];
        check!(
            projects(&pool).await.unwrap()
                == listed
                    .into_iter()
                    .map(|name| (format!("/projects/{name}"), Some(ids[name])))
                    .collect::<Paths>()
        );
    }
}
