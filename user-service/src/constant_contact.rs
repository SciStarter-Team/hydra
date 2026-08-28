//! Everything this service tells Constant Contact about a person.
//!
//! Keeping the whole conversation with that service in one module means there
//! is a single place where the contract lives: which properties they hold for
//! us, what those properties are called, and which endpoint is allowed to
//! write them.
//!
//! Two standing rules, both carried over from the orrery implementation this
//! replaces:
//!
//! Enrichment goes through `PUT /contacts/{id}`, never
//! `POST /contacts/sign_up_form`. The latter sets a contact's email permission
//! to `explicit` as a side effect and can resurrect an unsubscribed contact,
//! so it is correct for the act of subscribing and badly wrong as a way to
//! freshen somebody's first name.
//!
//! And nothing here creates a contact. Subscribing creates one, at the moment
//! a person asks to be subscribed; this sync only updates records that already
//! exist. An account Constant Contact does not hold is left alone, because
//! calling one into being so that there is something to enrich would add a
//! billable record and put an address that has never engaged onto a sending
//! list.

use std::{
    collections::{BTreeSet, HashMap},
    env::var,
    time::Duration,
};

use anyhow::{Error, anyhow};
use common::{
    message::log::{
        LastActiveForRequest, LastActiveForResponse, UsersActiveSinceRequest,
        UsersActiveSinceResponse,
    },
    message::user::normalize_tag,
    misc,
    service::request,
    shared::db,
};
use jiff::Timestamp;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::time::sleep;

const CONTACTS: &str = "https://api.cc.email/v3/contacts";
const CUSTOM_FIELDS: &str = "https://api.cc.email/v3/contact_custom_fields";
const CONTACT_TAGS: &str = "https://api.cc.email/v3/contact_tags";
const TOKEN_ENDPOINT: &str = "https://authz.constantcontact.com/oauth2/default/v1/token";

/// Their limit is four requests a second. This is the pacing every walk of the
/// user table against their API has always used.
pub const RATE_LIMIT_PAUSE: Duration = Duration::from_millis(330);

/// Their other limit is ten thousand requests a day, shared with anything else
/// using the same key -- including orrery, for as long as both are running.
/// Each account costs two: one to read the contact, one to write it back.
pub const SWEEP_BATCH: i64 = 2000;

/// The subresources `PUT /contacts/{id}` silently drops when they are absent
/// from the body, and so the ones a read-modify-write cycle has to carry.
const SUBRESOURCES: &str =
    "custom_fields,list_memberships,taggings,notes,phone_numbers,street_addresses,sms_channel";

/// Where the resolved ids for our custom fields and tags are kept, so that a
/// sweep does not re-list the account's vocabulary on every pass.
const FIELD_ID_KEY: &str = "constant-contact-fields";
const TAG_ID_KEY: &str = "constant-contact-tags";
const TOKEN_KEY: &str = "constant-contact-token";
const TOKEN_EXPIRY_KEY: &str = "constant-contact-token-expires";

/// Whether this service, rather than orrery, is the one pushing to Constant
/// Contact.
///
/// Their request allowance is per API key, so the two share it. Two sweeps
/// against one budget would contend and double-spend, so exactly one of them
/// pushes at a time.
///
/// Flipping this to `true` is one step of a cutover that also needs:
///
/// * orrery's `sweep_constant_contact` task unscheduled, so it stops pushing;
/// * `CONSTANTCONTACT_KEY`, `CONSTANTCONTACT_SECRET` and
///   `CONSTANTCONTACT_TOKEN` in this service's environment;
/// * and every account flagged once, because orrery may have pushed something
///   whose digest never reached us through the user sync:
///
///   ```sql
///   UPDATE users SET constant_contact_dirty = 1;
///   ```
///
///   Safe and self-limiting rather than a month of re-sending: the digest
///   comparison suppresses every unchanged account before any request is made,
///   so only genuinely stale records cost anything.
///
/// A const rather than an environment variable on purpose. The cutover means a
/// deploy regardless, and a switch that lives beside the thing it governs is
/// harder to forget than one in a config file.
const PRIMARY: bool = false;

/// Constant Contact allows five hundred tags per account. Stopping short of
/// that leaves room for tags staff make by hand, and means a runaway prefix
/// family fails loudly here rather than by exhausting the budget.
const TAG_CREATE_CEILING: usize = 400;

/// The daily request allowance is spent.
///
/// Separated from ordinary failure so that a sweep stops the moment it runs
/// out rather than spending the rest of its batch collecting 429s, which are
/// themselves requests and so dig the hole deeper. Whatever it did not reach
/// stays flagged, and the next run resumes there.
#[derive(Debug)]
pub struct QuotaExhausted;

impl std::fmt::Display for QuotaExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Constant Contact's daily request quota is spent; it resets at 00:00 UTC"
        )
    }
}

impl std::error::Error for QuotaExhausted {}

/// How one account's push turned out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PushResult {
    Updated,
    /// Flagged, but nothing Constant Contact holds had actually changed.
    Unchanged,
    /// Not one of their contacts, and this sync never makes one.
    Absent,
    /// Protected, and so never sent anywhere.
    Withheld,
    Failed,
}

/// One of our custom fields, as Constant Contact knows it.
///
/// `key` names the field in our code and never changes. `label` is what staff
/// see in the segment builder and the personalization menu; it can be renamed
/// there without breaking us, because we resolve to an id.
///
/// These labels must match orrery's `FIELDS` exactly for as long as both
/// systems run against the same account, since a label that differs by a
/// character creates a second field rather than finding the first.
pub struct CustomField {
    pub key: &'static str,
    pub label: &'static str,
}

pub const FIELDS: &[CustomField] = &[
    CustomField {
        key: "greeting_name",
        label: "Greeting name",
    },
    CustomField {
        key: "joined",
        label: "Joined SciStarter",
    },
    CustomField {
        key: "last_active",
        label: "Last active on site",
    },
    CustomField {
        key: "state",
        label: "State",
    },
    CustomField {
        key: "participated",
        label: "Projects participated in",
    },
    CustomField {
        key: "library",
        label: "Library affiliations",
    },
];

/// The tags Constant Contact is allowed to see.
///
/// Our tag vocabulary is open ended -- a go-link can mint one, and nothing
/// validates what arrives -- while their account holds only five hundred tags
/// in total. So the mirror works from an allowlist: a tag travels because it
/// is named here, not because it escaped a denylist. Tags marking staff are
/// absent deliberately, as are the high-cardinality families (`participated:`,
/// `library:`, `curated:`, `group:`) that travel as custom field values
/// instead, where cardinality is free.
///
/// Kept in step with orrery's `_ALWAYS_MIRRORED`.
pub const MIRRORED_TAGS: &[&str] = &[
    // Badges
    "foundations-badge",
    "data-literacy-badge",
    "libraries-hubs-badge",
    "data-ethics-badge",
    "higher-ed-badge",
    "k12-educators-badge",
    "ideal-badge",
    "1ma-badge",
    // Training. `training` and `training-old` mark distinct things and are
    // both carried; neither supersedes the other.
    "training",
    "training-old",
    "scistarter-champion-training-complete",
    "scistarter-ambassador-training-complete",
    // Ambassador programme
    "scistarter-ambassador",
    "scistarter-ambassador-application-submitted",
    "scistarter-ambassador-verified",
    "solar-system-ambassador",
    // Programmes and partners
    "girlscout",
    "girlscout-volunteer",
    "evolveme",
    "clever",
    "nasa_globe",
    "noaa",
    "verizon-index",
    "scistarter_partner",
    "scistarter_blog_user",
    "google_analytics_user",
    "curated-analytics",
    "broward",
    "ncsu",
    "csm2025",
    "csm2026-certificate",
    "citsciday",
    "citscimonth2020",
    "museum-of-science-boston",
    // Engagement
    "first-contribution",
    "project-page-member",
    // Acquisition
    "facebook-signup",
    "google-signup",
    "twitter-signup",
    "linkedin-signup",
    "facebook-login",
    "google-login",
    "twitter-login",
    "linkedin-login",
    "signup-turnstile-fallback",
];

/// Families whose members are all worth mirroring, so that next year's
/// `1ma-badge-2028` and the next partner embed arrive without an edit here.
pub const MIRRORED_TAG_PREFIXES: &[&str] = &["1ma-badge-", "signup-surface:", "partner-embed:"];

pub fn mirrored(tag: &str) -> bool {
    MIRRORED_TAGS.contains(&tag)
        || MIRRORED_TAG_PREFIXES
            .iter()
            .any(|prefix| tag.starts_with(prefix))
}

/// The tags in a column holding orrery's `||one||two||` tag encoding.
///
/// Orrery builds that string from a set, so the order it arrives in means
/// nothing. Everything downstream of here sorts before it matters.
pub fn decode_tags(raw: &str) -> BTreeSet<String> {
    raw.split("||")
        .map(normalize_tag)
        .filter(|tag| !tag.is_empty())
        .collect()
}

/// Write tags back in that same encoding, so that a column this service fills
/// in reads the same to orrery as one orrery filled in itself.
///
/// Empty for no tags rather than the `||||` orrery would write for an empty
/// set: both decode to nothing, and the empty string is what the column
/// defaults to.
pub fn encode_tags<S: AsRef<str>>(tags: impl IntoIterator<Item = S>) -> String {
    let names: BTreeSet<String> = tags
        .into_iter()
        .map(|tag| normalize_tag(tag.as_ref()))
        .filter(|tag| !tag.is_empty())
        .collect();

    if names.is_empty() {
        return String::new();
    }

    format!(
        "||{}||",
        names.into_iter().collect::<Vec<String>>().join("||")
    )
}

/// A bearer token, refreshed and cached until shortly before it expires.
async fn token() -> Result<String, Error> {
    let held: Option<String> = misc::get(TOKEN_KEY).await?;
    let expires: Option<String> = misc::get(TOKEN_EXPIRY_KEY).await?;

    if let (Some(held), Some(expires)) = (held, expires) {
        if expires
            .parse::<Timestamp>()
            .is_ok_and(|at| at > Timestamp::now())
        {
            return Ok(held);
        }
    }

    let key = var("CONSTANTCONTACT_KEY")?;
    let secret = var("CONSTANTCONTACT_SECRET")?;
    let refresh = var("CONSTANTCONTACT_TOKEN")?;

    let response = reqwest::Client::new()
        .post(TOKEN_ENDPOINT)
        .basic_auth(key, Some(secret))
        .form(&[
            ("refresh_token", refresh),
            ("grant_type", "refresh_token".into()),
        ])
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Renewing the Constant Contact token failed: {}",
            response.status()
        ));
    }

    let body: Value = response.json().await?;

    let granted = body
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Token response carried no access_token"))?
        .to_string();

    // Held a little short of the stated lifetime, so a sweep never picks up a
    // token that expires mid-batch.
    let seconds = body
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600)
        - 300;
    let until = (Timestamp::now() + Duration::from_secs(seconds.max(60) as u64)).to_string();

    misc::set(TOKEN_KEY, granted.clone()).await?;
    misc::set(TOKEN_EXPIRY_KEY, until).await?;

    Ok(granted)
}

/// Whether a response says the allowance, rather than this one request, is the
/// problem. Their 429 carries `throttled` for the per-second limit and
/// `quota_exceeded` for the daily one, and only the latter is worth stopping
/// for.
fn quota_spent(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS && body.contains("quota_exceeded")
}

/// A JSON request, with the quota check applied to whatever comes back.
async fn call(
    method: reqwest::Method,
    url: &str,
    body: Option<Value>,
) -> Result<Option<Value>, Error> {
    let mut request = reqwest::Client::new()
        .request(method, url)
        .bearer_auth(token().await?);

    if let Some(body) = body {
        request = request.json(&body);
    }

    let response = request.send().await?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();

    if quota_spent(status, &text) {
        return Err(QuotaExhausted.into());
    }

    if !status.is_success() {
        eprintln!("Constant Contact rejected {url}: {status} {text}");
        return Ok(None);
    }

    Ok(serde_json::from_str(&text).ok())
}

/// Our field keys mapped to their ids, creating any field the account does not
/// have yet.
pub async fn field_ids() -> Result<Option<HashMap<String, String>>, Error> {
    if let Some(known) = cached_ids(FIELD_ID_KEY).await? {
        if FIELDS.iter().all(|field| known.contains_key(field.key)) {
            return Ok(Some(known));
        }
    }

    let listing = match call(
        reqwest::Method::GET,
        &format!("{CUSTOM_FIELDS}?limit=100"),
        None,
    )
    .await?
    {
        Some(listing) => listing,
        None => return Ok(None),
    };

    let mut by_label = HashMap::new();

    for entry in listing
        .get("custom_fields")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if let (Some(label), Some(id)) = (
            entry.get("label").and_then(Value::as_str),
            entry.get("custom_field_id").and_then(Value::as_str),
        ) {
            by_label.insert(label.to_string(), id.to_string());
        }
    }

    let mut ids = HashMap::new();

    for field in FIELDS {
        if let Some(id) = by_label.get(field.label) {
            ids.insert(field.key.to_string(), id.clone());
            continue;
        }

        // Every field carried here is either text or a date, and Constant
        // Contact's date type gains us nothing our own comparisons need, so
        // both go over as strings -- matching what orrery created.
        let created = call(
            reqwest::Method::POST,
            CUSTOM_FIELDS,
            Some(json!({"label": field.label, "type": field_type(field.key)})),
        )
        .await?;

        match created
            .as_ref()
            .and_then(|body| body.get("custom_field_id"))
            .and_then(Value::as_str)
        {
            Some(id) => {
                ids.insert(field.key.to_string(), id.to_string());
            }
            None => return Ok(None),
        }
    }

    store_ids(FIELD_ID_KEY, &ids).await?;

    Ok(Some(ids))
}

fn field_type(key: &str) -> &'static str {
    match key {
        "joined" | "last_active" => "date",
        _ => "string",
    }
}

/// Ids for `names`, creating any tag the account does not have yet.
pub async fn tag_ids(names: &[String]) -> Result<Option<HashMap<String, String>>, Error> {
    let known = cached_ids(TAG_ID_KEY).await?.unwrap_or_default();

    if names.iter().all(|name| known.contains_key(name)) {
        return Ok(Some(known));
    }

    // A name we have not seen may still exist over there, made by hand or by
    // an earlier run whose cache was cleared, so look before creating.
    let listing = match call(
        reqwest::Method::GET,
        &format!("{CONTACT_TAGS}?limit=500"),
        None,
    )
    .await?
    {
        Some(listing) => listing,
        None => return Ok(None),
    };

    let mut ids = HashMap::new();

    for entry in listing
        .get("tags")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if let (Some(name), Some(id)) = (
            entry.get("name").and_then(Value::as_str),
            entry.get("tag_id").and_then(Value::as_str),
        ) {
            ids.insert(name.to_string(), id.to_string());
        }
    }

    for name in names {
        if ids.contains_key(name) {
            continue;
        }

        if ids.len() >= TAG_CREATE_CEILING {
            eprintln!(
                "Tag budget: refusing to create {name:?}; the account already holds {} tags",
                ids.len()
            );
            continue;
        }

        let created = call(
            reqwest::Method::POST,
            CONTACT_TAGS,
            Some(json!({"name": name})),
        )
        .await?;

        if let Some(id) = created
            .as_ref()
            .and_then(|body| body.get("tag_id"))
            .and_then(Value::as_str)
        {
            ids.insert(name.clone(), id.to_string());
        }
    }

    store_ids(TAG_ID_KEY, &ids).await?;

    Ok(Some(ids))
}

async fn cached_ids(key: &str) -> Result<Option<HashMap<String, String>>, Error> {
    let held: Option<String> = misc::get(key).await?;

    Ok(held.and_then(|held| serde_json::from_str(&held).ok()))
}

async fn store_ids(key: &str, ids: &HashMap<String, String>) -> Result<(), Error> {
    misc::set(key, serde_json::to_string(ids)?).await?;

    Ok(())
}

/// A fingerprint of everything we would send about an account.
///
/// The wire format is spelled out rather than left to a JSON encoder because
/// orrery computes these same digests in Python: sorted `key<tab>value` lines
/// joined by newlines, SHA-256 of the UTF-8 bytes, an absent value written as
/// the empty string rather than any spelling of null. A digest the two cannot
/// agree on would make every one of a hundred thousand accounts look changed
/// at the handover, which is three weeks of the daily allowance spent
/// re-sending what Constant Contact already holds.
pub fn digest_of(described: &[(&str, String)]) -> String {
    let mut lines: Vec<String> = described
        .iter()
        .map(|(key, value)| format!("{key}\t{value}"))
        .collect();

    lines.sort();

    let mut hasher = Sha256::new();
    hasher.update(lines.join("\n").as_bytes());

    hex::encode(hasher.finalize())
}

/// Of the account's tags, the ids of those this mirror is responsible for on
/// one person's contact.
///
/// Narrower than everything in `known`, which after a listing holds the whole
/// account: a tag someone made by hand is not ours to take away, so only names
/// covered by the mirror policy are managed. Per-person rather than global
/// because the policy is: this account's own signup context is part of what the
/// mirror put there, so it is also part of what the mirror may remove, while
/// the same name on somebody else's contact is none of its business.
pub fn managed_tag_ids<'a>(
    known: &'a HashMap<String, String>,
    signup_tags: &BTreeSet<String>,
) -> Vec<&'a String> {
    known
        .iter()
        .filter(|(name, _)| mirrored(name) || signup_tags.contains(name.as_str()))
        .map(|(_, id)| id)
        .collect()
}

/// `theirs` with the tags this mirror manages brought into line with `ours`,
/// and every other tag on the contact left untouched.
pub fn merged_taggings(
    theirs: &[String],
    ours: &[String],
    known: &HashMap<String, String>,
    signup_tags: &BTreeSet<String>,
) -> Vec<String> {
    let managed = managed_tag_ids(known, signup_tags);

    let mut mine: Vec<String> = ours
        .iter()
        .filter_map(|name| known.get(name).cloned())
        .collect();

    mine.sort();
    mine.dedup();

    theirs
        .iter()
        .filter(|id| !managed.contains(id))
        .cloned()
        .chain(mine)
        .collect()
}

/// `addresses` with the postal code applied to the first one, or a new address
/// carrying it if the contact has none.
///
/// Constant Contact allows one address per `kind`, so this edits in place
/// rather than appending and colliding.
pub fn with_postal_code(addresses: &[Value], postal_code: &str) -> Vec<Value> {
    let mut addresses: Vec<Value> = addresses.to_vec();

    match addresses.first_mut() {
        Some(Value::Object(first)) => {
            first.insert("postal_code".into(), json!(postal_code));
        }
        _ => {
            addresses = vec![json!({"kind": "home", "postal_code": postal_code})];
        }
    }

    addresses
}

/// Bring Constant Contact's copy of one account up to date.
pub async fn push(account: &Account) -> Result<PushResult, Error> {
    if account.protected {
        // A protected account's data never leaves the systems that promise to
        // protect it. Cleared with an empty digest so the sweep stops asking.
        mark_clean(&account.user_id, "").await?;
        return Ok(PushResult::Withheld);
    }

    let digest = account.digest();

    if digest == account.digest_sent {
        // Flagged, but nothing they hold changed. Most changes land here.
        mark_clean(&account.user_id, &digest).await?;
        return Ok(PushResult::Unchanged);
    }

    let contact_id = match account.contact_id.as_deref() {
        Some(id) if !id.is_empty() => id,
        // Not one of their contacts, and no record is created for them.
        // Cleared with an empty digest rather than the real one: the flag stops
        // the sweep spending a lookup on this account every run, while the
        // empty digest -- which no real fingerprint can equal -- guarantees a
        // full push if they ever do appear over there.
        _ => {
            mark_clean(&account.user_id, "").await?;
            return Ok(PushResult::Absent);
        }
    };

    let ids = match field_ids().await? {
        Some(ids) => ids,
        None => return Ok(PushResult::Failed),
    };

    let ours = account.mirrored_tags();
    let known = match tag_ids(&ours).await? {
        Some(known) => known,
        None => return Ok(PushResult::Failed),
    };

    let current = match call(
        reqwest::Method::GET,
        &format!("{CONTACTS}/{contact_id}?include={SUBRESOURCES}"),
        None,
    )
    .await?
    {
        Some(Value::Object(contact)) => contact,
        _ => return Ok(PushResult::Failed),
    };

    let mut contact = current;

    // PUT replaces the whole contact, so anything not restated is dropped.
    // Merge our fields over whatever is already on the record, and leave any
    // field we do not manage exactly where it was.
    let mut fields: Vec<Value> = contact
        .get("custom_fields")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|entry| {
            entry
                .get("custom_field_id")
                .and_then(Value::as_str)
                .is_none_or(|id| !ids.values().any(|ours| ours == id))
        })
        .cloned()
        .collect();

    for (key, value) in account.described() {
        // A field we have no value for is omitted rather than blanked, so that
        // a value entered on their side is left alone instead of overwritten
        // with nothing.
        if value.is_empty() || key == "tags" {
            continue;
        }

        if let Some(id) = ids.get(key) {
            fields.push(json!({"custom_field_id": id, "value": value}));
        }
    }

    contact.insert("custom_fields".into(), json!(fields));
    contact.insert("update_source".into(), json!("Account"));

    let theirs: Vec<String> = contact
        .get("taggings")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|id| id.as_str().map(str::to_string))
        .collect();

    contact.insert(
        "taggings".into(),
        json!(merged_taggings(
            &theirs,
            &ours,
            &known,
            &account.signup_tags
        )),
    );

    if !account.first_name.trim().is_empty() {
        contact.insert("first_name".into(), json!(account.first_name.trim()));
    }

    if !account.last_name.trim().is_empty() {
        contact.insert("last_name".into(), json!(account.last_name.trim()));
    }

    if !account.zipcode.trim().is_empty() {
        let addresses = contact
            .get("street_addresses")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        contact.insert(
            "street_addresses".into(),
            json!(with_postal_code(&addresses, account.zipcode.trim())),
        );
    }

    if call(
        reqwest::Method::PUT,
        &format!("{CONTACTS}/{contact_id}"),
        Some(Value::Object(contact)),
    )
    .await?
    .is_none()
    {
        return Ok(PushResult::Failed);
    }

    mark_clean(&account.user_id, &digest).await?;

    Ok(PushResult::Updated)
}

/// One account, as far as Constant Contact is concerned.
///
/// `last_active` is supplied rather than read from a column: activity is
/// log-service's concept and lives nowhere else, so the sweep asks that service
/// for the batch it is about to push and passes the answers in.
pub struct Account {
    pub user_id: String,
    pub protected: bool,
    pub first_name: String,
    pub last_name: String,
    pub handle: String,
    pub zipcode: String,
    pub state: String,
    pub joined: String,
    pub contact_id: Option<String>,
    pub digest_sent: String,
    pub tags: Vec<String>,
    /// Which of `tags` arrived from a signup context. Those travel whatever
    /// they are called, so a campaign link can put its own name in front of the
    /// people building the mailings without anyone editing the allowlist.
    pub signup_tags: BTreeSet<String>,
    pub last_active: String,
}

impl Account {
    /// The name to greet this person by, or nothing when there is none usable.
    ///
    /// Their first name if we have one, otherwise the handle they chose for
    /// themselves. A handle holding an email address is refused: it is not a
    /// name, and "Hi someone@example.com" reads worse than no personalization
    /// at all. Deliberately shorter than the chains behind the site header,
    /// which fall through to the address and then the account id.
    pub fn greeting_name(&self) -> String {
        let first = self.first_name.trim();

        if !first.is_empty() {
            return first.to_string();
        }

        let handle = self.handle.trim();

        if !handle.is_empty() && !handle.contains('@') {
            return handle.to_string();
        }

        String::new()
    }

    /// The tags on this account that Constant Contact is allowed to see.
    ///
    /// The allowlist and the prefixed families, plus whatever this account's
    /// own signup context supplied. That last part is what lets an open-ended
    /// tag travel without opening the allowlist to everything: a partner who
    /// sends people in under `?tag=their-campaign` gets that campaign in front
    /// of the people building the mailings, and it reaches only the accounts
    /// that actually arrived that way, so the tag Constant Contact gains means
    /// what the partner meant by it.
    pub fn mirrored_tags(&self) -> Vec<String> {
        let mut mine: Vec<String> = self
            .tags
            .iter()
            .filter(|tag| mirrored(tag) || self.signup_tags.contains(tag.as_str()))
            .cloned()
            .collect();

        mine.sort();

        mine
    }

    /// Values from a prefixed tag family, as one space-separated field.
    ///
    /// Capped because a custom field value is finite and the cap is complete
    /// for very nearly everyone -- the ninety-ninth percentile is six. The
    /// handful above it are better served by an audience built to order than
    /// by a field that silently loses its tail.
    fn family(&self, prefix: &str, limit: usize) -> String {
        let mut values: Vec<&str> = self
            .tags
            .iter()
            .filter_map(|tag| tag.strip_prefix(prefix))
            .collect();

        values.sort();
        values.truncate(limit);

        values.join(" ")
    }

    /// Everything we would send, keyed the way the digest expects.
    ///
    /// Must stay in step with orrery's `digest_of`: same keys, same values,
    /// same empty-string-for-absent convention. The keys are the field keys
    /// plus `tags`, `first_name`, `last_name` and `zipcode`.
    pub fn described(&self) -> Vec<(&'static str, String)> {
        vec![
            ("greeting_name", self.greeting_name()),
            ("joined", date_part(&self.joined)),
            ("last_active", date_part(&self.last_active)),
            ("state", self.state.trim().to_string()),
            ("participated", self.family("participated:", 8)),
            ("library", self.family("library:", 8)),
            ("tags", self.mirrored_tags().join(" ")),
            ("first_name", self.first_name.trim().to_string()),
            ("last_name", self.last_name.trim().to_string()),
            ("zipcode", self.zipcode.trim().to_string()),
        ]
    }

    pub fn digest(&self) -> String {
        digest_of(&self.described())
    }
}

/// The date part of a timestamp, since these fields are dates rather than
/// moments.
///
/// This is what keeps the sweep cheap: someone browsing all day moves their
/// last-active value at most once, so the digest does not churn and the same
/// account is not re-sent on every pass.
fn date_part(stamp: &str) -> String {
    stamp
        .split(['T', ' '])
        .next()
        .unwrap_or_default()
        .to_string()
}

/// The accounts whose Constant Contact copy has fallen behind.
///
/// Bounded per call because their API is rate limited: the queue drains over
/// however many runs it takes, and whatever is left stays flagged.
pub async fn pending(limit: i64) -> Result<Vec<Account>, Error> {
    let rows = sqlx::query_as!(
        Stored,
        r#"select
             "user_id" as "user_id!",
             "protected" as "protected!",
             "first_name" as "first_name!",
             "last_name" as "last_name!",
             "handle" as "handle!",
             "zipcode",
             "state" as "state!",
             "date_joined" as "date_joined!",
             "constant_contact_id",
             "constant_contact_digest" as "constant_contact_digest!",
             "signup_tags" as "signup_tags!"
           from "users"
           where "constant_contact_dirty" = 1
             and "newsletter" = 1
             and "protected" = 0
           order by "user_id"
           limit ?"#,
        limit,
    )
    .fetch_all(db())
    .await?;

    let mut accounts = Vec::with_capacity(rows.len());

    for row in rows {
        accounts.push(row.into_account().await?);
    }

    Ok(accounts)
}

/// One account, found by any of the addresses it can be reached at.
///
/// For driving a single account by hand rather than waiting for a sweep.
pub async fn account_for_email(email: &str) -> Result<Option<Account>, Error> {
    let row = sqlx::query_as!(
        Stored,
        r#"select
             "users"."user_id" as "user_id!",
             "users"."protected" as "protected!",
             "users"."first_name" as "first_name!",
             "users"."last_name" as "last_name!",
             "users"."handle" as "handle!",
             "users"."zipcode",
             "users"."state" as "state!",
             "users"."date_joined" as "date_joined!",
             "users"."constant_contact_id",
             "users"."constant_contact_digest" as "constant_contact_digest!",
             "users"."signup_tags" as "signup_tags!"
           from "emails"
           join "users" on "users"."user_id" = "emails"."user_id"
           where "emails"."email" = ?
           limit 1"#,
        email,
    )
    .fetch_optional(db())
    .await?;

    match row {
        Some(row) => Ok(Some(row.into_account().await?)),
        None => Ok(None),
    }
}

/// The stored half of an account: everything the users table holds that
/// Constant Contact cares about. Tags and the last-active date come from
/// elsewhere, so they are filled in afterwards.
struct Stored {
    user_id: String,
    protected: i64,
    first_name: String,
    last_name: String,
    handle: String,
    zipcode: Option<String>,
    state: String,
    date_joined: String,
    constant_contact_id: Option<String>,
    constant_contact_digest: String,
    signup_tags: String,
}

impl Stored {
    async fn into_account(self) -> Result<Account, Error> {
        let tags = sqlx::query_scalar!(
            r#"select "tag" from "user_tags" where "user_id" = ?"#,
            self.user_id,
        )
        .fetch_all(db())
        .await?;

        Ok(Account {
            user_id: self.user_id,
            protected: self.protected != 0,
            first_name: self.first_name,
            last_name: self.last_name,
            handle: self.handle,
            zipcode: self.zipcode.unwrap_or_default(),
            state: self.state,
            joined: self.date_joined,
            contact_id: self.constant_contact_id,
            digest_sent: self.constant_contact_digest,
            tags,
            signup_tags: decode_tags(&self.signup_tags),
            // Filled in by the caller from log-service, which is the only place
            // activity is recorded.
            last_active: String::new(),
        })
    }
}

/// Record that Constant Contact is up to date for an account.
async fn mark_clean(user_id: &str, digest: &str) -> Result<(), Error> {
    sqlx::query!(
        r#"update "users"
           set "constant_contact_dirty" = 0, "constant_contact_digest" = ?
           where "user_id" = ?"#,
        digest,
        user_id,
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Flag an account for the next sweep.
pub async fn mark_dirty(user_id: &str) -> Result<(), Error> {
    sqlx::query!(
        r#"update "users" set "constant_contact_dirty" = 1 where "user_id" = ?"#,
        user_id,
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Name the US state an account sits in, from the location already on it.
///
/// Reverse geocoding rather than a table of state outlines: this service
/// already does that, with a cache keyed to a rounded coordinate, so accounts
/// sharing a postal-code centroid share one lookup. Accounts synced from orrery
/// arrive with their state already named, so the volume here is new signups --
/// tens a day, nearly all of them cache hits.
///
/// Leaves the state alone rather than blanking it if the location is missing or
/// the lookup fails: a name already on the row is better than none.
pub async fn derive_state(user_id: &str) -> Result<(), Error> {
    let located = sqlx::query!(
        r#"select
             ST_X("point") as "longitude?: f64",
             ST_Y("point") as "latitude?: f64",
             "country_code"
           from "users"
           where "user_id" = ? and "point" is not null"#,
        user_id,
    )
    .fetch_optional(db())
    .await?;

    let Some(located) = located else {
        return Ok(());
    };

    let (Some(longitude), Some(latitude)) = (located.longitude, located.latitude) else {
        return Ok(());
    };

    // The field is a US state, so there is nothing to name for an address
    // elsewhere, and no reason to spend a lookup discovering that.
    if !located.country_code.eq_ignore_ascii_case("US") {
        return Ok(());
    }

    let found = crate::geocode::reverse_lookup(latitude, longitude).await?;

    let state = found.state.trim();

    if state.is_empty() {
        return Ok(());
    }

    sqlx::query!(
        r#"update "users"
           set "state" = ?, "constant_contact_dirty" = 1
           where "user_id" = ?"#,
        state,
        user_id,
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Name the state without making the person wait for a geocoding call.
pub fn derive_state_in_background(user_id: String) {
    tokio::spawn(async move {
        if let Err(err) = derive_state(&user_id).await {
            eprintln!("Could not name the state for {user_id}: {err:?}");
        }
    });
}

/// The e-newsletter list, and the list that stands for having opted out.
///
/// The same two identifiers orrery uses. Not secrets, and stable for years, so
/// they sit here rather than in the environment.
const NEWSLETTER_LIST: &str = "65cbb8c0-4c88-11e3-a08b-d4ae527b77f8";
const UNSUBSCRIBED_LIST: &str = "88c00a90-1606-11e5-a47e-d4ae527547e4";

const SIGN_UP_FORM: &str = "https://api.cc.email/v3/contacts/sign_up_form";

/// Whether an address belongs to a protected account.
///
/// Checked by address rather than by account, so that an address reachable at a
/// protected person's inbox is covered even when the caller has no account in
/// hand -- the same guarantee orrery's `is_protected_address` makes.
pub async fn protected_address(email: &str) -> Result<bool, Error> {
    let held = sqlx::query_scalar!(
        r#"select count(*) from "emails"
           join "users" on "users"."user_id" = "emails"."user_id"
           where "emails"."email" = ? and "users"."protected" = 1"#,
        email,
    )
    .fetch_one(db())
    .await?;

    Ok(held > 0)
}

/// Subscribe an address to the newsletter, creating their contact.
///
/// The one operation that legitimately brings a contact into being: it happens
/// because a person asked to be subscribed. `POST /contacts/sign_up_form` is
/// the right endpoint for exactly this and no other purpose -- it asserts
/// explicit permission to send, which is true here and would be a lie anywhere
/// else.
///
/// Only the subscription is done here. The details Constant Contact holds are
/// left to the sweep, which reaches the account within twelve hours: assembling
/// the payload needs the custom field ids and a last-active date from
/// log-service, and a person creating an account should not wait on either.
/// That also keeps one place, rather than two, that decides what gets sent.
///
/// Returns whether they are now subscribed. A refusal is not an error: the
/// caller records it by clearing `newsletter`, so our own record never claims a
/// subscription Constant Contact did not take.
pub async fn subscribe(user_id: &str, email: &str) -> Result<bool, Error> {
    // A protected account's address is never handed over: doing so publishes it
    // beyond the systems that promise to protect it.
    if protected_address(email).await? {
        eprintln!("Refusing to subscribe a protected account");
        return Ok(false);
    }

    let created = call(
        reqwest::Method::POST,
        SIGN_UP_FORM,
        Some(json!({
            "email_address": email,
            "list_memberships": [NEWSLETTER_LIST],
        })),
    )
    .await?;

    let Some(body) = created else {
        return Ok(false);
    };

    let contact_id = body
        .get("contact_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if contact_id.is_empty() {
        return Ok(false);
    }

    // Taken because it is free: the alternative is the sweep spending a lookup
    // to learn the same thing.
    sqlx::query!(
        r#"update "users" set "constant_contact_id" = ? where "user_id" = ?"#,
        contact_id,
        user_id,
    )
    .execute(db())
    .await?;

    // Somebody re-subscribing may still be on the list that stands for having
    // opted out. The sign-up endpoint unions list memberships rather than
    // replacing them, so coming off that one is a separate edit.
    leave_list(&contact_id, UNSUBSCRIBED_LIST).await?;

    Ok(true)
}

/// Take an address off the newsletter list and put it on the one that stands
/// for having opted out.
pub async fn unsubscribe(email: &str) -> Result<bool, Error> {
    let changed = call(
        reqwest::Method::POST,
        SIGN_UP_FORM,
        Some(json!({
            "email_address": email,
            "list_memberships": [UNSUBSCRIBED_LIST],
        })),
    )
    .await?;

    let Some(body) = changed else {
        return Ok(false);
    };

    let Some(contact_id) = body.get("contact_id").and_then(Value::as_str) else {
        return Ok(false);
    };

    leave_list(contact_id, NEWSLETTER_LIST).await?;

    Ok(true)
}

/// Remove one list membership, leaving everything else on the contact alone.
///
/// A whole read-modify-write because `PUT` replaces the contact: anything not
/// restated is dropped.
async fn leave_list(contact_id: &str, list_id: &str) -> Result<(), Error> {
    let current = match call(
        reqwest::Method::GET,
        &format!("{CONTACTS}/{contact_id}?include={SUBRESOURCES}"),
        None,
    )
    .await?
    {
        Some(Value::Object(contact)) => contact,
        _ => return Ok(()),
    };

    let mut contact = current;

    let remaining: Vec<Value> = contact
        .get("list_memberships")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|held| held.as_str() != Some(list_id))
        .cloned()
        .collect();

    contact.insert("list_memberships".into(), json!(remaining));
    // The person asked for this, so it is their doing rather than ours.
    contact.insert("update_source".into(), json!("Contact"));

    call(
        reqwest::Method::PUT,
        &format!("{CONTACTS}/{contact_id}"),
        Some(Value::Object(contact)),
    )
    .await?;

    Ok(())
}

/// Subscribe without making the person wait for it.
///
/// Three requests to a rate-limited service has no business inside the response
/// to a signup. If it fails, `newsletter` is cleared: our own record claiming a
/// subscription that Constant Contact never took is the failure mode worth
/// avoiding, and it is exactly the one orrery had for years.
pub fn subscribe_in_background(user_id: String, email: String) {
    tokio::spawn(async move {
        match subscribe(&user_id, &email).await {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                if let Err(err) = sqlx::query!(
                    r#"update "users" set "newsletter" = 0 where "user_id" = ?"#,
                    user_id,
                )
                .execute(db())
                .await
                {
                    eprintln!("Could not record a failed subscription: {err:?}");
                }
            }
        }
    });
}

/// Permissions that mean a person has told Constant Contact to stop.
///
/// Their say-so through that channel is the one that counts, so it outranks our
/// own newsletter column. `unsubscribed` is somebody asking us to stop;
/// `temp_hold` is a delivery problem and `pending_confirmation` an unfinished
/// double opt-in, which are different things wearing the same result, and all
/// three mean we should not be mailing them.
const OPTED_OUT: &[&str] = &["unsubscribed", "temp_hold", "pending_confirmation"];

/// Walk every contact Constant Contact holds and bring our record of who is on
/// the list into line with theirs.
///
/// Consent is only ever withdrawn here, never granted: a contact they hold as
/// active does not cause us to set `newsletter`, because our column may have
/// been cleared deliberately. An unsubscribe made through one of their emails,
/// on the other hand, is the one that counts legally, and nothing else carries
/// it back to us.
///
/// One paginated walk rather than a question per account: their listing is
/// hundreds of pages, which is a couple of minutes, where asking about a
/// hundred thousand accounts individually would be weeks of the allowance.
pub async fn reconcile() -> Result<(), Error> {
    let mut url = Some(format!("{CONTACTS}?limit=500&status=all"));
    let mut pages = 0;
    let mut held = 0;
    let mut departed = 0;
    let mut identified = 0;

    while let Some(fetching) = url {
        let body = match call(reqwest::Method::GET, &fetching, None).await? {
            Some(body) => body,
            // A partial walk is not a partial answer: stopping here leaves our
            // records as they were, where carrying on would be reading "they do
            // not hold these people" into pages we never fetched.
            None => {
                return Err(anyhow!("Contact enumeration failed at page {pages}"));
            }
        };

        for contact in body
            .get("contacts")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let address = contact.get("email_address");

            let (Some(contact_id), Some(email)) = (
                contact.get("contact_id").and_then(Value::as_str),
                address
                    .and_then(|address| address.get("address"))
                    .and_then(Value::as_str),
            ) else {
                continue;
            };

            let permission = address
                .and_then(|address| address.get("permission_to_send"))
                .and_then(Value::as_str)
                .unwrap_or_default();

            held += 1;

            // Matched through the emails table, which holds every address an
            // account may be reached at rather than just its primary.
            let matched = sqlx::query!(
                r#"select "users"."user_id" as "user_id!",
                          "users"."newsletter",
                          "users"."constant_contact_id"
                   from "emails"
                   join "users" on "users"."user_id" = "emails"."user_id"
                   where "emails"."email" = ?
                   limit 1"#,
                email,
            )
            .fetch_optional(db())
            .await?;

            let Some(account) = matched else {
                continue;
            };

            if account.constant_contact_id.as_deref() != Some(contact_id) {
                sqlx::query!(
                    r#"update "users" set "constant_contact_id" = ? where "user_id" = ?"#,
                    contact_id,
                    account.user_id,
                )
                .execute(db())
                .await?;

                identified += 1;
            }

            if OPTED_OUT.contains(&permission) && account.newsletter != 0 {
                sqlx::query!(
                    r#"update "users" set "newsletter" = 0 where "user_id" = ?"#,
                    account.user_id,
                )
                .execute(db())
                .await?;

                departed += 1;
            }
        }

        pages += 1;

        url = body
            .get("_links")
            .and_then(|links| links.get("next"))
            .and_then(|next| next.get("href"))
            .and_then(Value::as_str)
            .map(|href| format!("https://api.cc.email{href}"));

        if url.is_some() {
            sleep(RATE_LIMIT_PAUSE).await;
        }
    }

    println!(
        "Constant Contact reconciliation: {held} contacts across {pages} pages, \
         {identified} contact ids recorded, {departed} accounts they had already dropped"
    );

    Ok(())
}

/// Reconcile once a week, which is the pace at which an unsubscribe made over
/// there matters.
///
/// Cheap enough not to need pacing against the sweep: a few hundred requests
/// against a ten thousand daily allowance.
pub async fn reconcile_periodically() {
    if !PRIMARY {
        return;
    }

    loop {
        if let Err(err) = reconcile().await {
            eprintln!("Constant Contact reconciliation failed: {err:?}");
        }

        sleep(Duration::from_secs(60 * 60 * 24 * 7)).await;
    }
}

/// Where the sweep remembers how far it has read log-service's activity, so a
/// run only asks about accounts seen since the last one.
const ACTIVITY_BOOKMARK: &str = "constant-contact-activity-bookmark";

/// Flag every account log-service has seen since the last sweep.
///
/// Activity is log-service's concept and lives nowhere else, so this is a poll
/// rather than a column: the sweep asks who moved, and only then does it have
/// a reason to recompute anybody's digest.
async fn flag_the_newly_active() -> Result<usize, Error> {
    let since: Option<String> = misc::get(ACTIVITY_BOOKMARK).await?;
    // The moment the question is asked, not the moment it is answered, so an
    // account seen while the sweep runs is caught by the next one rather than
    // missed by both.
    let asked_at = Timestamp::now().to_string();

    let answer: UsersActiveSinceResponse = request(
        "log.users_active_since",
        UsersActiveSinceRequest {
            since: since.unwrap_or_else(|| String::from("2000-01-01T00:00:00Z")),
        },
    )
    .await?;

    for user_id in &answer.user_ids {
        mark_dirty(user_id).await?;
    }

    misc::set(ACTIVITY_BOOKMARK, asked_at).await?;

    Ok(answer.user_ids.len())
}

/// The last-active date for a batch, from the only service that knows.
async fn last_active_for(accounts: &[Account]) -> Result<HashMap<String, String>, Error> {
    let answer: LastActiveForResponse = request(
        "log.last_active_for",
        LastActiveForRequest {
            user_ids: accounts
                .iter()
                .map(|account| account.user_id.clone())
                .collect(),
        },
    )
    .await?;

    Ok(answer
        .entries
        .into_iter()
        .map(|entry| (entry.user_id, entry.when))
        .collect())
}

/// Push the accounts whose Constant Contact copy has fallen behind.
///
/// Bounded per run because their allowance is ten thousand requests a day and
/// each account costs two. Whatever this run does not reach stays flagged, so
/// the next one resumes there -- which is also what makes stopping on a spent
/// quota lossless.
pub async fn sweep(limit: i64) -> Result<(), Error> {
    match flag_the_newly_active().await {
        Ok(0) => {}
        Ok(seen) => println!("Constant Contact sweep: {seen} accounts have been active"),
        // Not fatal: the accounts already flagged are still worth pushing, and
        // the bookmark is left where it was so nothing is skipped.
        Err(err) => eprintln!("Could not ask which accounts have been active: {err:?}"),
    }

    let mut accounts = pending(limit).await?;

    if accounts.is_empty() {
        return Ok(());
    }

    let seen = last_active_for(&accounts).await?;

    for account in &mut accounts {
        if let Some(when) = seen.get(&account.user_id) {
            account.last_active = when.clone();
        }
    }

    let mut counts: HashMap<PushResult, usize> = HashMap::new();

    for account in &accounts {
        match push(account).await {
            Ok(outcome) => {
                *counts.entry(outcome).or_default() += 1;
            }
            Err(err) if err.is::<QuotaExhausted>() => {
                println!("Constant Contact sweep stopped: {err}");
                break;
            }
            Err(err) => {
                eprintln!("Pushing {} failed: {err:?}", account.user_id);
                *counts.entry(PushResult::Failed).or_default() += 1;
            }
        }

        sleep(RATE_LIMIT_PAUSE).await;
    }

    println!("Constant Contact sweep: {counts:?}");

    Ok(())
}

/// Run the sweep twice a day, for as long as this service is the one doing the
/// pushing.
///
/// Two runs of two thousand accounts is eight thousand of the ten thousand
/// daily requests, leaving room for whatever else uses the same key --
/// including orrery, for as long as both are running.
pub async fn sweep_periodically() {
    if !PRIMARY {
        println!(
            "Constant Contact: orrery is still the primary, so not sweeping. \
             See PRIMARY in constant_contact.rs for the cutover."
        );
        return;
    }

    loop {
        if let Err(err) = sweep(SWEEP_BATCH).await {
            eprintln!("Constant Contact sweep failed: {err:?}");
        }

        sleep(Duration::from_secs(60 * 60 * 12)).await;
    }
}

/// One-off operations, for driving this by hand rather than waiting on a sweep.
///
/// Reached as `user-service constant-contact <what> [argument]`. A subcommand of
/// the service rather than a binary of its own because this crate has no library
/// target, and giving it one to gain a second binary would mean restructuring a
/// service that works.
pub async fn command(what: Option<&str>, argument: Option<&str>) {
    match (what, argument) {
        (Some("describe"), Some(email)) => describe(email).await,
        (Some("push"), Some(email)) => match account_for_email(email).await {
            Ok(Some(mut account)) => {
                if let Ok(seen) = last_active_for(std::slice::from_ref(&account)).await {
                    if let Some(when) = seen.get(&account.user_id) {
                        account.last_active = when.clone();
                    }
                }

                match push(&account).await {
                    Ok(outcome) => println!("{outcome:?}"),
                    Err(err) => eprintln!("Failed: {err:?}"),
                }
            }
            Ok(None) => eprintln!("No account is reachable at {email}"),
            Err(err) => eprintln!("Failed: {err:?}"),
        },
        (Some("sweep"), limit) => {
            let limit = limit.and_then(|n| n.parse().ok()).unwrap_or(SWEEP_BATCH);

            if let Err(err) = sweep(limit).await {
                eprintln!("Failed: {err:?}");
            }
        }
        (Some("reconcile"), _) => {
            if let Err(err) = reconcile().await {
                eprintln!("Failed: {err:?}");
            }
        }
        _ => eprintln!(
            "usage: user-service constant-contact describe <email>\n\
             \x20      user-service constant-contact push <email>\n\
             \x20      user-service constant-contact sweep [limit]\n\
             \x20      user-service constant-contact reconcile"
        ),
    }
}

/// Print what would be sent for one account, without contacting anybody.
///
/// The last-active date is the one part that needs asking for, and it comes from
/// log-service rather than from Constant Contact.
async fn describe(email: &str) {
    let found = match account_for_email(email).await {
        Ok(Some(account)) => account,
        Ok(None) => {
            eprintln!("No account is reachable at {email}");
            return;
        }
        Err(err) => {
            eprintln!("Failed: {err:?}");
            return;
        }
    };

    let mut account = found;

    match last_active_for(std::slice::from_ref(&account)).await {
        Ok(seen) => {
            if let Some(when) = seen.get(&account.user_id) {
                account.last_active = when.clone();
            }
        }
        Err(err) => eprintln!("(could not reach log-service for last-active: {err})"),
    }

    let labels: HashMap<&str, &str> = FIELDS
        .iter()
        .map(|field| (field.key, field.label))
        .collect();

    println!("{}", account.user_id);

    for (key, value) in account.described() {
        if key == "tags" {
            continue;
        }

        let label = labels.get(key).copied().unwrap_or(key);

        println!(
            "  {label}: {}",
            if value.is_empty() { "(none)" } else { &value }
        );
    }

    let mirrored = account.mirrored_tags();
    println!(
        "  tags: {}",
        if mirrored.is_empty() {
            String::from("(none)")
        } else {
            mirrored.join(" ")
        }
    );

    let withheld: Vec<&String> = account
        .tags
        .iter()
        .filter(|tag| !mirrored.contains(tag))
        .collect();

    if !withheld.is_empty() {
        println!(
            "  withheld: {}",
            withheld
                .iter()
                .map(|tag| tag.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
    }

    println!("  contact: {:?}", account.contact_id);
    println!("  digest now: {}", account.digest());
    println!("  digest sent: {}", account.digest_sent);
    println!("Nothing sent.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> Account {
        Account {
            user_id: "01JGEDS64C0ECBWR472V7788BF".into(),
            protected: false,
            first_name: "Ada".into(),
            last_name: "Lovelace".into(),
            handle: "countess".into(),
            zipcode: "02139".into(),
            state: "Massachusetts".into(),
            joined: "2019-04-01T12:00:00Z".into(),
            contact_id: Some("cc-1".into()),
            digest_sent: String::new(),
            tags: vec![
                "foundations-badge".into(),
                "training".into(),
                "scistarter-staff".into(),
                "participated:globe-observer".into(),
                "participated:crowd-the-tap".into(),
                "library:verizon-index".into(),
            ],
            signup_tags: BTreeSet::new(),
            last_active: "2026-07-29T08:15:00Z".into(),
        }
    }

    /// The digest orrery computes for exactly this description, taken from a
    /// run of its `digest_of_description`.
    ///
    /// This test is the contract between the two implementations. If it fails,
    /// the two no longer agree, and every account would look changed when this
    /// service takes over the sweep -- three weeks of the daily request
    /// allowance spent re-sending what Constant Contact already holds. Fix the
    /// divergence rather than re-recording the constant.
    const ORRERY_DIGEST: &str = "b4d3216f6e6b763d5739c23b7b476e15e5d164bbac2cb96c766fdf6e3173727a";

    #[test]
    fn the_digest_matches_the_one_orrery_computes() {
        assert_eq!(account().digest(), ORRERY_DIGEST);
    }

    #[test]
    fn a_first_name_is_preferred_to_a_handle() {
        assert_eq!(account().greeting_name(), "Ada");
    }

    #[test]
    fn the_handle_stands_in_for_a_missing_first_name() {
        let mut person = account();
        person.first_name = String::new();

        assert_eq!(person.greeting_name(), "countess");
    }

    #[test]
    fn a_handle_holding_an_email_address_is_refused() {
        let mut person = account();
        person.first_name = String::new();
        person.handle = "jmnorman@hotmail.invalid".into();

        assert_eq!(person.greeting_name(), "");
    }

    #[test]
    fn staff_tags_are_withheld() {
        assert!(
            !account()
                .mirrored_tags()
                .contains(&"scistarter-staff".to_string())
        );
    }

    #[test]
    fn training_and_training_old_are_both_carried() {
        let mut person = account();
        person.tags = vec!["training".into(), "training-old".into()];

        assert_eq!(person.mirrored_tags(), vec!["training", "training-old"]);
    }

    /// A tag from a signup context travels whatever it is called, so that a
    /// campaign link can put its own name in front of the people building the
    /// mailings without anyone editing the allowlist first.
    #[test]
    fn a_tag_from_the_signup_context_is_carried() {
        let mut person = account();
        person.tags = vec!["their-campaign".into()];
        person.signup_tags = BTreeSet::from(["their-campaign".to_string()]);

        assert_eq!(person.mirrored_tags(), vec!["their-campaign"]);
    }

    /// The vocabulary is per account, not global: the same name means what the
    /// partner meant by it only on the accounts that arrived under it.
    #[test]
    fn the_same_tag_on_somebody_else_is_still_withheld() {
        let mut stranger = account();
        stranger.tags = vec!["their-campaign".into()];

        assert!(stranger.mirrored_tags().is_empty());
    }

    #[test]
    fn a_signup_tag_the_account_has_since_lost_is_not_sent() {
        let mut person = account();
        person.tags = vec![];
        person.signup_tags = BTreeSet::from(["their-campaign".to_string()]);

        assert!(person.mirrored_tags().is_empty());
    }

    #[test]
    fn the_mirror_may_take_back_a_signup_tag_it_placed() {
        let known = HashMap::from([
            ("their-campaign".to_string(), "ours".to_string()),
            ("hand-made-by-staff".to_string(), "theirs".to_string()),
        ]);

        let merged = merged_taggings(
            &["theirs".to_string(), "ours".to_string()],
            &[],
            &known,
            &BTreeSet::from(["their-campaign".to_string()]),
        );

        assert_eq!(merged, vec!["theirs"]);
    }

    /// The encoding is a contract with orrery, which writes the same column.
    #[test]
    fn tag_columns_round_trip_through_orrerys_encoding() {
        assert_eq!(
            decode_tags("||one||two||"),
            BTreeSet::from(["one".to_string(), "two".to_string()])
        );

        // What orrery writes for an empty set, and what an untouched column
        // holds.
        assert!(decode_tags("||||").is_empty());
        assert!(decode_tags("").is_empty());

        assert_eq!(
            encode_tags(["Two", " one ", "#three"]),
            "||one||three||two||"
        );
        assert_eq!(encode_tags(["", "   "]), "");
        assert_eq!(
            decode_tags(&encode_tags(["A", "b"])),
            decode_tags("||a||b||")
        );
    }

    /// A tag is spelled one way, not two.
    ///
    /// `user_tags` and the `signup_tags` column are written by different code,
    /// and an account's signup tags are recognized by matching the two against
    /// each other. Were the spellings to drift apart, the tag would simply stay
    /// out of the mirror, with nothing anywhere looking wrong.
    #[test]
    fn a_tag_is_spelled_the_same_in_both_columns() {
        for raw in [" #Spring-Push ", "GirlScout", "their-campaign", "#a"] {
            let held = normalize_tag(raw);

            assert_eq!(
                decode_tags(&encode_tags([raw])),
                BTreeSet::from([held.clone()]),
                "{raw:?} reaches signup_tags as something other than {held:?}"
            );
        }

        // Punctuation and space is not a tag at all, on either side.
        assert!(normalize_tag(" ## ").is_empty());
        assert!(encode_tags([" ## "]).is_empty());
    }

    /// The ambassador path tags and the mirror have to agree.
    ///
    /// The mirror treats every name it lists as ours to manage, so a path tag
    /// missing from the mirror would go unpushed, and a mirrored ambassador tag
    /// that nothing writes would be stripped from every contact holding it on
    /// the first push after the sweep changes hands. Neither failure is visible
    /// from either side alone.
    #[test]
    fn every_ambassador_tag_we_write_is_mirrored() {
        for tag in crate::ambassador::PATH_PAST_CHAMPION
            .iter()
            .chain(&[common::tag::CHAMPION_TRAINING])
        {
            assert!(mirrored(tag), "{tag} is written but not mirrored");
        }
    }

    /// The other direction, which is the one that loses data.
    ///
    /// A mirrored tag nothing writes gets stripped from every contact holding
    /// it on the first push after the sweep changes hands, because the merge
    /// reads its absence as the person having lost it. One tag is legitimately
    /// in that position -- the retired spelling of the Champion training tag,
    /// which is read but never written -- so it is named here rather than left
    /// to weaken the check for everything else.
    #[test]
    fn no_mirrored_ambassador_tag_goes_unwritten_by_accident() {
        let written: Vec<&str> = crate::ambassador::PATH_PAST_CHAMPION
            .iter()
            .copied()
            .chain(common::tag::CHAMPION_TRAINING_ANY.iter().copied())
            .collect();

        for tag in MIRRORED_TAGS {
            if tag.starts_with("scistarter-ambassador") || tag.starts_with("scistarter-champion") {
                assert!(written.contains(tag), "{tag} is mirrored but never written");
            }
        }
    }

    #[test]
    fn a_prefixed_family_is_carried_without_naming_each_member() {
        assert!(mirrored("1ma-badge-2028"));
        assert!(mirrored("signup-surface:embed"));
        assert!(!mirrored("group:44"));
    }

    #[test]
    fn high_cardinality_families_travel_as_fields_not_tags() {
        let described = account().described();
        let field = |key| {
            described
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.clone())
                .unwrap()
        };

        assert_eq!(field("participated"), "crowd-the-tap globe-observer");
        assert_eq!(field("library"), "verizon-index");
        assert_eq!(field("tags"), "foundations-badge training");
    }

    #[test]
    fn the_participation_field_is_capped() {
        let mut person = account();
        person.tags = (0..12)
            .map(|index| format!("participated:project-{index:02}"))
            .collect();

        assert_eq!(person.family("participated:", 8).split(' ').count(), 8);
    }

    #[test]
    fn dates_are_sent_without_their_time() {
        let described = account().described();

        assert!(described.contains(&("joined", "2019-04-01".to_string())));
        assert!(described.contains(&("last_active", "2026-07-29".to_string())));
    }

    #[test]
    fn the_merge_leaves_tags_we_do_not_manage_alone() {
        let known = HashMap::from([
            ("foundations-badge".to_string(), "ours".to_string()),
            ("hand-made-by-staff".to_string(), "theirs".to_string()),
        ]);

        let merged = merged_taggings(
            &["theirs".to_string()],
            &["foundations-badge".to_string()],
            &known,
            &BTreeSet::new(),
        );

        assert_eq!(merged, vec!["theirs", "ours"]);
    }

    #[test]
    fn the_merge_removes_a_managed_tag_the_person_has_lost() {
        let known = HashMap::from([
            ("foundations-badge".to_string(), "ours".to_string()),
            ("hand-made-by-staff".to_string(), "theirs".to_string()),
        ]);

        let merged = merged_taggings(
            &["theirs".to_string(), "ours".to_string()],
            &[],
            &known,
            &BTreeSet::new(),
        );

        assert_eq!(merged, vec!["theirs"]);
    }

    #[test]
    fn a_postal_code_lands_on_the_existing_address() {
        let addresses = with_postal_code(&[json!({"kind": "home", "city": "Cambridge"})], "02139");

        assert_eq!(addresses.len(), 1);
        assert_eq!(addresses[0]["city"], "Cambridge");
        assert_eq!(addresses[0]["postal_code"], "02139");
    }

    #[test]
    fn a_postal_code_creates_an_address_when_there_is_none() {
        let addresses = with_postal_code(&[], "02139");

        assert_eq!(
            addresses,
            vec![json!({"kind": "home", "postal_code": "02139"})]
        );
    }

    #[test]
    fn a_spent_quota_is_told_apart_from_one_throttled_request() {
        let too_many = reqwest::StatusCode::TOO_MANY_REQUESTS;

        assert!(quota_spent(too_many, r#"{"error_key":"quota_exceeded"}"#));
        assert!(!quota_spent(too_many, r#"{"error_key":"throttled"}"#));
    }
}
