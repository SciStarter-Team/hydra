//! The lists projects' and events' multiple-choice fields pick from, which
//! staff keep at `/-admin/choice-lists`: an event's format, the program or
//! campaign it is part of, and its kind of venue, which together say what
//! kind of event it is; and topics, activities and audience. Events and
//! projects share the Topic and Audience lists.
//!
//! A value is keyed within its list by a slug made from its name when the
//! value is made, which never changes after, so renaming a value leaves what
//! picks it alone. What a project or event picks is a set of slugs, written
//! through [`assign`] and read through the `choice_names` view.
//!
//! Until cutover the lists are copied from orrery ([`ChoiceListCopy`]), which
//! keys its values by id. Each value orrery has carries that id, which is how
//! the copy, the syncs and the orrery-compatible APIs find it; orrery's names
//! can change, so nothing matches on them once a value is here. Orrery keeps
//! the three lists of event types as one ([`OrreryList`]).

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap},
};

use anyhow::Error;
use common::{shared::db, transaction::Immediate};

use crate::peering::LoadStorePeered;

/// The program Acts of Science Connected events pick, which the Citizen
/// Science Month finder marks as signature events and can be narrowed to.
pub const ACTS_OF_SCIENCE_CONNECTED: (ChoiceList, &str) =
    (ChoiceList::EventProgram, "acts-of-science-connected");

/// A list of values to choose from.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    sqlx::Type,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum ChoiceList {
    EventFormat,
    EventProgram,
    EventVenue,
    Topic,
    Activity,
    Audience,
}

impl ChoiceList {
    pub const ALL: &'static [ChoiceList] = &[
        ChoiceList::EventFormat,
        ChoiceList::EventProgram,
        ChoiceList::EventVenue,
        ChoiceList::Topic,
        ChoiceList::Activity,
        ChoiceList::Audience,
    ];

    /// The lists that together say what kind of event an event is, in the
    /// order they are asked. The `event_type_lists` view names the same
    /// lists, in the same order, for SQL.
    pub const EVENT_TYPES: [ChoiceList; 3] = [
        ChoiceList::EventFormat,
        ChoiceList::EventProgram,
        ChoiceList::EventVenue,
    ];

    /// What the list is called, for staff.
    pub fn label(self) -> &'static str {
        match self {
            ChoiceList::EventFormat => "Event formats",
            ChoiceList::EventProgram => "Event programs and campaigns",
            ChoiceList::EventVenue => "Event venues",
            ChoiceList::Topic => "Topics",
            ChoiceList::Activity => "Activities",
            ChoiceList::Audience => "Audience",
        }
    }

    /// The question the list answers, as forms and pages head what is picked
    /// from it.
    pub fn heading(self) -> &'static str {
        match self {
            ChoiceList::EventFormat => "Format",
            ChoiceList::EventProgram => "Program or campaign",
            ChoiceList::EventVenue => "Venue",
            ChoiceList::Topic => "Topics",
            ChoiceList::Activity => "Activities",
            ChoiceList::Audience => "Audience",
        }
    }

    /// The list's segment of `/-admin/choice-lists/…`.
    pub fn path(self) -> &'static str {
        match self {
            ChoiceList::EventFormat => "event-formats",
            ChoiceList::EventProgram => "event-programs",
            ChoiceList::EventVenue => "event-venues",
            ChoiceList::Topic => "topics",
            ChoiceList::Activity => "activities",
            ChoiceList::Audience => "audience",
        }
    }

    pub fn from_path(path: &str) -> Option<ChoiceList> {
        ChoiceList::ALL
            .iter()
            .copied()
            .find(|list| list.path() == path)
    }

    /// The orrery list the list is copied from.
    pub fn orrery_list(self) -> OrreryList {
        match self {
            ChoiceList::EventFormat | ChoiceList::EventProgram | ChoiceList::EventVenue => {
                OrreryList::EventType
            }
            ChoiceList::Topic => OrreryList::Topics,
            ChoiceList::Activity => OrreryList::Activities,
            ChoiceList::Audience => OrreryList::Audience,
        }
    }

    /// The facet orrery gives the values of its list that belong in this one:
    /// a value's facet says which question an orrery event type answers, and
    /// the values of orrery's other lists have none.
    pub fn orrery_facet(self) -> &'static str {
        match self {
            ChoiceList::EventFormat => "format",
            ChoiceList::EventProgram => "program",
            ChoiceList::EventVenue => "venue",
            ChoiceList::Topic | ChoiceList::Activity | ChoiceList::Audience => "",
        }
    }
}

/// One of orrery's lists (its `Choice`s), which hydra's are copied from until
/// cutover. Orrery keeps an event's format, program and venue in one list of
/// event types, its values each with a facet saying which of those questions
/// it answers; hydra keeps each in a list of its own.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum OrreryList {
    EventType,
    Topics,
    Activities,
    Audience,
}

impl OrreryList {
    pub const ALL: &'static [OrreryList] = &[
        OrreryList::EventType,
        OrreryList::Topics,
        OrreryList::Activities,
        OrreryList::Audience,
    ];

    /// The name of the orrery `Choice`.
    pub fn name(self) -> &'static str {
        match self {
            OrreryList::EventType => "Event type",
            OrreryList::Topics => "Topics",
            OrreryList::Activities => "Activities",
            OrreryList::Audience => "Audience",
        }
    }

    pub fn from_name(name: &str) -> Option<OrreryList> {
        OrreryList::ALL
            .iter()
            .copied()
            .find(|list| list.name() == name)
    }

    /// The lists copied from it.
    pub fn lists(self) -> &'static [ChoiceList] {
        match self {
            OrreryList::EventType => &ChoiceList::EVENT_TYPES,
            OrreryList::Topics => &[ChoiceList::Topic],
            OrreryList::Activities => &[ChoiceList::Activity],
            OrreryList::Audience => &[ChoiceList::Audience],
        }
    }

    /// The list a value of this one with `facet` belongs in, if any does.
    pub fn list_for(self, facet: &str) -> Option<ChoiceList> {
        self.lists()
            .iter()
            .copied()
            .find(|list| list.orrery_facet() == facet)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChoiceValue {
    pub list: ChoiceList,
    pub slug: String,
    pub name: String,

    /// Where the value comes in its list, lowest first. Ties are broken by
    /// name.
    pub position: i64,

    /// Orrery's id for the value, for one orrery has.
    pub orrery_id: Option<i64>,
}

impl ChoiceValue {
    /// Every value of the list, in list order.
    pub async fn all(list: ChoiceList) -> Result<Vec<ChoiceValue>, Error> {
        Self::all_from(db(), list).await
    }

    async fn all_from<'e>(
        executor: impl sqlx::SqliteExecutor<'e>,
        list: ChoiceList,
    ) -> Result<Vec<ChoiceValue>, Error> {
        Ok(sqlx::query_as!(
            ChoiceValue,
            r#"SELECT "list" AS "list: ChoiceList", "slug", "name", "position", "orrery_id"
               FROM choice_values WHERE "list" = ? ORDER BY "position", "name""#,
            list
        )
        .fetch_all(executor)
        .await?)
    }

    /// The value of the list named `name`, ignoring case.
    pub async fn named(list: ChoiceList, name: &str) -> Result<Option<ChoiceValue>, Error> {
        Ok(sqlx::query_as!(
            ChoiceValue,
            r#"SELECT "list" AS "list: ChoiceList", "slug", "name", "position", "orrery_id"
               FROM choice_values WHERE "list" = ? AND "name" = ?"#,
            list,
            name
        )
        .fetch_optional(db())
        .await?)
    }

    /// The value of the list whose slug is `slug`.
    pub async fn with_slug(list: ChoiceList, slug: &str) -> Result<Option<ChoiceValue>, Error> {
        Ok(sqlx::query_as!(
            ChoiceValue,
            r#"SELECT "list" AS "list: ChoiceList", "slug", "name", "position", "orrery_id"
               FROM choice_values WHERE "list" = ? AND "slug" = ?"#,
            list,
            slug
        )
        .fetch_optional(db())
        .await?)
    }

    /// The position after the last value's, for a value added to the end of
    /// the list.
    pub async fn next_position(list: ChoiceList) -> Result<i64, Error> {
        Ok(sqlx::query_scalar!(
            r#"SELECT coalesce(max("position") + 1, 0) AS "next!: i64"
               FROM choice_values WHERE "list" = ?"#,
            list
        )
        .fetch_one(db())
        .await?)
    }

    /// Those of `slugs` that name a value of the list, so that what a form
    /// submits can only pick values that exist.
    pub async fn known(
        list: ChoiceList,
        slugs: impl IntoIterator<Item = String>,
    ) -> Result<BTreeSet<String>, Error> {
        let slugs = serde_json::to_string(&slugs.into_iter().collect::<Vec<_>>())?;

        Ok(sqlx::query_scalar!(
            r#"SELECT "slug" AS "slug!" FROM choice_values
               WHERE "list" = ? AND "slug" IN (SELECT "value" FROM json_each(?))"#,
            list,
            slugs
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .collect())
    }

    /// The names of the values among `slugs`, in list order.
    pub async fn names(list: ChoiceList, slugs: &BTreeSet<String>) -> Result<Vec<String>, Error> {
        let slugs = serde_json::to_string(slugs)?;

        Ok(sqlx::query_scalar!(
            r#"SELECT "name" AS "name!" FROM choice_values
               WHERE "list" = ? AND "slug" IN (SELECT "value" FROM json_each(?))
               ORDER BY "position", "name""#,
            list,
            slugs
        )
        .fetch_all(db())
        .await?)
    }

    /// The slugs of the values of the list that `target`, a project or event,
    /// picks.
    pub async fn of_target(list: ChoiceList, target: &str) -> Result<BTreeSet<String>, Error> {
        Ok(sqlx::query_scalar!(
            r#"SELECT "value" AS "value!" FROM choice_names WHERE "target" = ? AND "list" = ?"#,
            target,
            list
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .collect())
    }

    /// The values of the list that `target` picks, in list order.
    pub async fn picked_by(list: ChoiceList, target: &str) -> Result<Vec<ChoiceValue>, Error> {
        Ok(sqlx::query_as!(
            ChoiceValue,
            r#"SELECT v."list" AS "list: ChoiceList", v."slug", v."name", v."position", v."orrery_id"
               FROM choice_assignments a
               JOIN choice_values v ON v."list" = a."list" AND v."slug" = a."value"
               WHERE a."target" = ? AND a."list" = ?
               ORDER BY v."position", v."name""#,
            target,
            list
        )
        .fetch_all(db())
        .await?)
    }

    /// The names of the values of the list that `target` picks, in list
    /// order.
    pub async fn names_of_target(list: ChoiceList, target: &str) -> Result<Vec<String>, Error> {
        Ok(Self::picked_by(list, target)
            .await?
            .into_iter()
            .map(|value| value.name)
            .collect())
    }

    /// How many projects and events pick each value of the list, by slug. A
    /// value nothing picks is absent.
    pub async fn usage(list: ChoiceList) -> Result<HashMap<String, i64>, Error> {
        Ok(sqlx::query!(
            r#"SELECT "value" AS "value!", count(*) AS "uses!: i64"
               FROM choice_assignments WHERE "list" = ? GROUP BY "value""#,
            list
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| (row.value, row.uses))
        .collect())
    }

    /// The slugs of the values copied from `from` that orrery knows by
    /// `ids`, by the list each is in here; every list copied from `from` is
    /// present, if only empty. An id none of them has is reported and left
    /// out.
    pub async fn slugs_of_orrery_ids(
        from: OrreryList,
        ids: &[i64],
    ) -> Result<BTreeMap<ChoiceList, BTreeSet<String>>, Error> {
        let lists = serde_json::to_string(from.lists())?;
        let wanted = serde_json::to_string(ids)?;

        let found = sqlx::query!(
            r#"SELECT "list" AS "list: ChoiceList", "slug", "orrery_id" AS "orrery_id!"
               FROM choice_values
               WHERE "list" IN (SELECT "value" FROM json_each(?))
                 AND "orrery_id" IN (SELECT "value" FROM json_each(?))"#,
            lists,
            wanted
        )
        .fetch_all(db())
        .await?;

        for id in ids {
            if !found.iter().any(|row| row.orrery_id == *id) {
                tracing::warn!("No list copied from orrery's {from:?} has a value with id {id}");
            }
        }

        let mut slugs: BTreeMap<ChoiceList, BTreeSet<String>> = from
            .lists()
            .iter()
            .map(|list| (*list, BTreeSet::new()))
            .collect();
        for row in found {
            slugs.entry(row.list).or_default().insert(row.slug);
        }

        Ok(slugs)
    }

    /// What each of `targets` picks from the list, in list order. A target
    /// that picks nothing is absent.
    pub async fn picked_by_each(
        list: ChoiceList,
        targets: &[String],
    ) -> Result<HashMap<String, Vec<ChoiceValue>>, Error> {
        let targets = serde_json::to_string(targets)?;

        let rows = sqlx::query!(
            r#"SELECT a."target" AS "target!", v."list" AS "list: ChoiceList", v."slug", v."name",
                      v."position", v."orrery_id"
               FROM choice_assignments a
               JOIN choice_values v ON v."list" = a."list" AND v."slug" = a."value"
               WHERE a."list" = ? AND a."target" IN (SELECT "value" FROM json_each(?))
               ORDER BY v."position", v."name""#,
            list,
            targets
        )
        .fetch_all(db())
        .await?;

        let mut picked: HashMap<String, Vec<ChoiceValue>> = HashMap::new();
        for row in rows {
            picked.entry(row.target).or_default().push(ChoiceValue {
                list: row.list,
                slug: row.slug,
                name: row.name,
                position: row.position,
                orrery_id: row.orrery_id,
            });
        }

        Ok(picked)
    }

    /// The values of the list that at least one project or event picks, in
    /// list order.
    pub async fn in_use(list: ChoiceList) -> Result<Vec<ChoiceValue>, Error> {
        Ok(sqlx::query_as!(
            ChoiceValue,
            r#"SELECT v."list" AS "list: ChoiceList", v."slug", v."name", v."position", v."orrery_id"
               FROM choice_values v
               WHERE v."list" = ?1
                 AND EXISTS (SELECT 1 FROM choice_assignments a
                             WHERE a."list" = ?1 AND a."value" = v."slug")
               ORDER BY v."position", v."name""#,
            list
        )
        .fetch_all(db())
        .await?)
    }

    /// The value among `values` that a partner names, as orrery's APIs accept
    /// one: by its orrery id, as a JSON integer, or by its exact name.
    pub fn partner_named<'v>(
        values: &'v [ChoiceValue],
        given: &serde_json::Value,
    ) -> Option<&'v ChoiceValue> {
        values.iter().find(|value| match given {
            serde_json::Value::Number(number) => {
                number.as_i64().is_some() && number.as_i64() == value.orrery_id
            }
            serde_json::Value::String(text) => *text == value.name,
            _ => false,
        })
    }

    /// The slugs of the values among `values` that a partner's field names. A
    /// list names one value per item; anything else names one value. Items
    /// that name nothing are dropped, as orrery drops them.
    pub fn partner_all_named(
        values: &[ChoiceValue],
        given: &serde_json::Value,
    ) -> BTreeSet<String> {
        let items = match given {
            serde_json::Value::Array(items) => items.as_slice(),
            single => std::slice::from_ref(single),
        };

        items
            .iter()
            .filter_map(|item| Self::partner_named(values, item))
            .map(|value| value.slug.clone())
            .collect()
    }

    /// The value as orrery's APIs give one: its orrery id beside its name.
    pub fn partner_json(&self, id_key: &str, name_key: &str) -> serde_json::Value {
        serde_json::json!({ id_key: self.orrery_id, name_key: self.name })
    }
}

/// Every list's values as they stood when loaded, for reading what partners
/// send, which names values by orrery id or name, without a query per value.
#[derive(Debug, Default, Clone)]
pub struct ChoiceLists(HashMap<ChoiceList, Vec<ChoiceValue>>);

impl ChoiceLists {
    pub async fn load() -> Result<Self, Error> {
        let mut lists = HashMap::new();
        for list in ChoiceList::ALL {
            lists.insert(*list, ChoiceValue::all(*list).await?);
        }
        Ok(ChoiceLists(lists))
    }

    /// The lists holding just `values`, in the order given.
    pub fn of(values: impl IntoIterator<Item = ChoiceValue>) -> Self {
        let mut lists: HashMap<ChoiceList, Vec<ChoiceValue>> = HashMap::new();
        for value in values {
            lists.entry(value.list).or_default().push(value);
        }
        ChoiceLists(lists)
    }

    /// The values of `list`, in list order.
    pub fn get(&self, list: ChoiceList) -> &[ChoiceValue] {
        self.0.get(&list).map_or(&[], Vec::as_slice)
    }
}

/// The heading of a field picking from a list, saying how many may be
/// picked when there is a `limit`.
pub fn limited_heading(title: &'static str, limit: Option<usize>) -> Cow<'static, str> {
    match limit {
        Some(limit) => format!("{title} (up to {limit})").into(),
        None => title.into(),
    }
}

/// Checkboxes for picking from a list, in list order, under a legend when
/// they are one of several lists in a fieldset of their own. With a limit,
/// once that many are ticked the rest are disabled, here and, as boxes are
/// ticked, by `js/pick-limit.js`.
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/choice_checkboxes.stpl.html")]
pub struct ChoiceCheckboxes {
    /// The form field, which is submitted as `field[]`.
    field: String,
    legend: Option<Cow<'static, str>>,
    values: Vec<ChoiceValue>,
    picked: BTreeSet<String>,
    limit: Option<usize>,
}

impl ChoiceCheckboxes {
    /// Checkboxes for `values`, all of one list, with `picked` ticked.
    pub fn new(
        field: impl Into<String>,
        values: Vec<ChoiceValue>,
        picked: BTreeSet<String>,
        limit: Option<usize>,
    ) -> Self {
        ChoiceCheckboxes {
            field: field.into(),
            legend: None,
            values,
            picked,
            limit,
        }
    }

    /// Checkboxes for every value of `list`.
    pub async fn load(
        field: impl Into<String>,
        list: ChoiceList,
        picked: BTreeSet<String>,
        limit: Option<usize>,
    ) -> Result<Self, Error> {
        Ok(Self::new(
            field,
            ChoiceValue::all(list).await?,
            picked,
            limit,
        ))
    }

    /// The boxes as a fieldset under `legend`.
    pub fn with_legend(mut self, legend: impl Into<Cow<'static, str>>) -> Self {
        self.legend = Some(legend.into());
        self
    }

    /// Whether as many are ticked as may be.
    fn full(&self) -> bool {
        self.limit.is_some_and(|limit| self.picked.len() >= limit)
    }
}

/// Make what `target` picks from the list exactly `slugs`, inside the
/// caller's transaction. Every write of a project's or event's choices goes
/// through here.
pub async fn assign(
    tx: &mut Immediate,
    target: &str,
    list: ChoiceList,
    slugs: &BTreeSet<String>,
) -> Result<(), Error> {
    sqlx::query!(
        r#"DELETE FROM choice_assignments WHERE "target" = ? AND "list" = ?"#,
        target,
        list
    )
    .execute(&mut *tx)
    .await?;

    for slug in slugs {
        sqlx::query!(
            r#"INSERT INTO choice_assignments ("target", "list", "value") VALUES (?, ?, ?)"#,
            target,
            list,
            slug
        )
        .execute(&mut *tx)
        .await?;
    }

    Ok(())
}

/// Take away every value the list's `slug` is picked by, inside the caller's
/// transaction, along with the value itself.
async fn remove(tx: &mut Immediate, list: ChoiceList, slug: &str) -> Result<(), Error> {
    sqlx::query!(
        r#"DELETE FROM choice_assignments WHERE "list" = ? AND "value" = ?"#,
        list,
        slug
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        r#"DELETE FROM choice_values WHERE "list" = ? AND "slug" = ?"#,
        list,
        slug
    )
    .execute(&mut *tx)
    .await?;

    Ok(())
}

impl LoadStorePeered for ChoiceValue {
    type DatabaseKey = (ChoiceList, String);

    const GROUP: &str = "choice_value";

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        sqlx::query!(
            r#"INSERT INTO choice_values ("list", "slug", "name", "position", "orrery_id")
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT ("list", "slug") DO UPDATE SET
                 "name" = excluded."name",
                 "position" = excluded."position",
                 "orrery_id" = excluded."orrery_id""#,
            self.list,
            self.slug,
            self.name,
            self.position,
            self.orrery_id
        )
        .execute(db())
        .await?;

        Ok(())
    }

    #[tracing::instrument]
    async fn read_from_database((list, slug): Self::DatabaseKey) -> Result<Self, Error> {
        Self::with_slug(list, &slug)
            .await?
            .ok_or_else(|| anyhow::anyhow!("{list:?} has no value {slug:?}"))
    }

    /// Takes the value away from everything that picks it, too.
    #[tracing::instrument]
    async fn delete_from_database((list, slug): Self::DatabaseKey) -> Result<(), Error> {
        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;
        remove(&mut tx, list, &slug).await?;
        tx.commit().await?;
        Ok(())
    }
}

/// One value of an orrery list, as orrery's `/staff/sync/choices` gives it,
/// and the list here it belongs in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OrreryChoice {
    pub id: i64,
    pub name: String,
    pub list: ChoiceList,
}

/// Orrery's whole list, in its order, which the lists here copied from it
/// are made to match: until cutover orrery's lists are the ones staff keep.
///
/// A value orrery has is found by its id, in whichever of the lists it is,
/// or else by name among the values of its list that orrery has not given an
/// id. It takes orrery's name and position, and moves to the list orrery's
/// facet for it says, taking what picks it along; it keeps its slug unless
/// that list has the slug already. A value orrery has no longer is deleted,
/// along with every pick of it. A value made here and unknown to orrery
/// keeps its name, and comes after orrery's in its list in the order they
/// were in, unless orrery renames or moves a value onto its name, when the
/// two are one value, orrery's.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChoiceListCopy {
    from: OrreryList,
    values: Vec<OrreryChoice>,
}

impl LoadStorePeered for ChoiceListCopy {
    type DatabaseKey = OrreryList;

    const GROUP: &str = "choice_list_copy";

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        self.save_into(db()).await
    }

    #[tracing::instrument]
    async fn read_from_database(from: Self::DatabaseKey) -> Result<Self, Error> {
        Self::read_from(db(), from).await
    }
}

impl ChoiceListCopy {
    /// The copy of `from` holding `values`, in orrery's order. Each value
    /// must belong in one of the lists copied from `from`; one that does not
    /// is reported and left out.
    pub fn new(from: OrreryList, values: Vec<OrreryChoice>) -> Self {
        let lists = from.lists();
        let mut values: Vec<OrreryChoice> = values
            .into_iter()
            .filter(|choice| {
                let belongs = lists.contains(&choice.list);
                if !belongs {
                    tracing::warn!(
                        "Orrery's {from:?} value {} belongs in {:?}, which is not copied from it",
                        choice.id,
                        choice.list
                    );
                }
                belongs
            })
            .collect();

        // Grouped by list, each keeping orrery's order, as the lists read
        // back.
        values.sort_by_key(|choice| lists.iter().position(|list| *list == choice.list));

        ChoiceListCopy { from, values }
    }

    async fn save_into(&self, pool: &sqlx::SqlitePool) -> Result<(), Error> {
        let lists = self.from.lists();

        // Read in the transaction, since each sync copies the lists before
        // its batch and two may copy at once.
        let mut tx = Immediate::begin_with_retry(pool, None, None).await?;
        let mut existing = Vec::new();
        for list in lists {
            existing.extend(ChoiceValue::all_from(&mut tx, *list).await?);
        }
        let key = |value: &ChoiceValue| (value.list, value.slug.clone());

        // Which value each of orrery's is, if it is one already here.
        let mut claimed: BTreeSet<(ChoiceList, String)> = BTreeSet::new();
        let mut matched: Vec<(&OrreryChoice, Option<&ChoiceValue>)> = Vec::new();
        for choice in &self.values {
            let found = existing
                .iter()
                .find(|value| value.orrery_id == Some(choice.id))
                .or_else(|| {
                    existing.iter().find(|value| {
                        value.list == choice.list
                            && value.orrery_id.is_none()
                            && !claimed.contains(&key(value))
                            && value.name.to_lowercase() == choice.name.to_lowercase()
                    })
                });
            if let Some(value) = found {
                claimed.insert(key(value));
            }
            matched.push((choice, found));
        }

        for value in &existing {
            if value.orrery_id.is_some() && !claimed.contains(&key(value)) {
                remove(&mut tx, value.list, &value.slug).await?;
            }
        }

        // Names are unique within a list, and orrery may have swapped two, or
        // moved one to a list holding its name, so every value that is
        // renamed or moved first sets its name aside.
        for (choice, found) in &matched {
            if let Some(value) = found
                && (value.name != choice.name || value.list != choice.list)
            {
                let aside = format!("\u{1}{}\u{1}{}", choice.id, value.slug);
                sqlx::query!(
                    r#"UPDATE choice_values SET "name" = ? WHERE "list" = ? AND "slug" = ?"#,
                    aside,
                    value.list,
                    value.slug
                )
                .execute(&mut tx)
                .await?;
            }
        }

        // Every slug in each list, including those of values about to move
        // out of it, so that no value is moved or made onto one.
        let mut taken: BTreeSet<(ChoiceList, String)> = existing
            .iter()
            .filter(|value| value.orrery_id.is_none() || claimed.contains(&key(value)))
            .map(key)
            .collect();
        let mut free_slug = |list: ChoiceList, wanted: &str, id: i64| {
            let slug = if wanted.is_empty() || taken.contains(&(list, wanted.to_string())) {
                format!("{}-{id}", if wanted.is_empty() { "value" } else { wanted })
            } else {
                wanted.to_string()
            };
            taken.insert((list, slug.clone()));
            slug
        };

        let mut positions: HashMap<ChoiceList, i64> = HashMap::new();
        for (choice, found) in &matched {
            let list = choice.list;
            let slug = match found {
                Some(value) if value.list == list => value.slug.clone(),
                Some(value) => {
                    let slug = free_slug(list, &value.slug, choice.id);
                    sqlx::query!(
                        r#"UPDATE choice_values SET "list" = ?, "slug" = ?
                           WHERE "list" = ? AND "slug" = ?"#,
                        list,
                        slug,
                        value.list,
                        value.slug
                    )
                    .execute(&mut tx)
                    .await?;
                    sqlx::query!(
                        r#"UPDATE OR REPLACE choice_assignments SET "list" = ?, "value" = ?
                           WHERE "list" = ? AND "value" = ?"#,
                        list,
                        slug,
                        value.list,
                        value.slug
                    )
                    .execute(&mut tx)
                    .await?;
                    slug
                }
                None => free_slug(list, &crate::slug::slugify(&choice.name), choice.id),
            };

            // A value made here that orrery's has been renamed or moved onto
            // the name of is the same value: what picks it picks orrery's
            // instead, and it goes.
            if let Some(duplicate) = existing.iter().find(|value| {
                value.list == list
                    && value.orrery_id.is_none()
                    && !claimed.contains(&key(value))
                    && value.name.to_lowercase() == choice.name.to_lowercase()
            }) {
                sqlx::query!(
                    r#"UPDATE OR REPLACE choice_assignments SET "value" = ?
                       WHERE "list" = ? AND "value" = ?"#,
                    slug,
                    list,
                    duplicate.slug
                )
                .execute(&mut tx)
                .await?;
                remove(&mut tx, list, &duplicate.slug).await?;
                claimed.insert(key(duplicate));
            }

            let position = positions.entry(list).or_default();
            sqlx::query!(
                r#"INSERT INTO choice_values ("list", "slug", "name", "position", "orrery_id")
                   VALUES (?, ?, ?, ?, ?)
                   ON CONFLICT ("list", "slug") DO UPDATE SET
                     "name" = excluded."name",
                     "position" = excluded."position",
                     "orrery_id" = excluded."orrery_id""#,
                list,
                slug,
                choice.name,
                *position,
                choice.id
            )
            .execute(&mut tx)
            .await?;
            *position += 1;
        }

        for list in lists {
            let after = positions.get(list).copied().unwrap_or_default();
            for (offset, value) in existing
                .iter()
                .filter(|value| {
                    value.list == *list
                        && value.orrery_id.is_none()
                        && !claimed.contains(&key(value))
                })
                .enumerate()
            {
                let position = after + offset as i64;
                sqlx::query!(
                    r#"UPDATE choice_values SET "position" = ? WHERE "list" = ? AND "slug" = ?"#,
                    position,
                    value.list,
                    value.slug
                )
                .execute(&mut tx)
                .await?;
            }
        }

        tx.commit().await?;

        Ok(())
    }

    async fn read_from(pool: &sqlx::SqlitePool, from: OrreryList) -> Result<Self, Error> {
        let mut values = Vec::new();
        for list in from.lists() {
            values.extend(
                ChoiceValue::all_from(pool, *list)
                    .await?
                    .into_iter()
                    .filter_map(|value| {
                        value.orrery_id.map(|id| OrreryChoice {
                            id,
                            name: value.name,
                            list: value.list,
                        })
                    }),
            );
        }

        Ok(ChoiceListCopy { from, values })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;
    use sqlx::{
        Row, SqlitePool,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };

    /// The migration that made the choice lists.
    const CHOICE_LISTS: i64 = 20261110195030;

    /// The migration that split the event types into three lists.
    const SPLIT_EVENT_TYPES: i64 = 20261110213547;

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

    /// A scratch database, migrated up to (but not including) `before`, or
    /// fully when it is `None`.
    async fn database(before: Option<i64>) -> SqlitePool {
        // SAFETY: mod_spatialite is a trusted extension, the same one the
        // application loads.
        let options = unsafe { SqliteConnectOptions::new().extension("mod_spatialite") };

        // One connection, because every connection to ":memory:" is a
        // database of its own.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.filename(":memory:"))
            .await
            .unwrap();

        // Made by common::shared::init before the migrations run.
        sqlx::query(
            r#"create table "misc" ("key" text primary key not null, "value" any not null) strict"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        migrate(&pool, |version| {
            before.is_none_or(|before| version < before)
        })
        .await;

        pool
    }

    /// Run the migrations whose versions `wanted` picks, in order.
    async fn migrate(pool: &SqlitePool, wanted: impl Fn(i64) -> bool) {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| wanted(migration.version))
        {
            sqlx::raw_sql(migration.sql.clone())
                .execute(pool)
                .await
                .unwrap_or_else(|err| panic!("migration {} failed: {err}", migration.version));
        }
    }

    /// Add an event with nothing but a name, saved as EventAll saves, with
    /// INSERT OR REPLACE; and with its type, while the events table still has
    /// one.
    async fn add_event(pool: &SqlitePool, event_id: &str, name: &str, event_type: Option<&str>) {
        let query = match event_type {
            Some(event_type) => sqlx::query(
                "INSERT OR REPLACE INTO events (event_id, created, flagged, admin_notes, welcomed,
                     page_creator_id, name, description, origin, presenter, url, image,
                     image_credit, how_to_join, location_type, entirely_online, location_name,
                     location_description, address, city, zip, state, country, event_type)
                 VALUES (?1, '2026-01-01T00:00:00Z', 0, '', 0, '', ?2, '', '', '', '', '', '', '',
                     '', 0, '', '', '', '', '', '', '', ?3)",
            )
            .bind(event_id)
            .bind(name)
            .bind(event_type),
            None => sqlx::query(
                "INSERT OR REPLACE INTO events (event_id, created, flagged, admin_notes, welcomed,
                     page_creator_id, name, description, origin, presenter, url, image,
                     image_credit, how_to_join, location_type, entirely_online, location_name,
                     location_description, address, city, zip, state, country)
                 VALUES (?1, '2026-01-01T00:00:00Z', 0, '', 0, '', ?2, '', '', '', '', '', '', '',
                     '', 0, '', '', '', '', '', '', '')",
            )
            .bind(event_id)
            .bind(name),
        };

        query.execute(pool).await.unwrap();
    }

    async fn picks(pool: &SqlitePool, target: &str) -> Vec<(String, String)> {
        sqlx::query(
            r#"SELECT "list", "value" FROM choice_assignments WHERE "target" = ? ORDER BY 1, 2"#,
        )
        .bind(target)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect()
    }

    fn pick(list: &str, value: &str) -> (String, String) {
        (list.to_string(), value.to_string())
    }

    /// What `events_fts` holds for the event, one entry per row.
    async fn indexed(pool: &SqlitePool, event_id: &str) -> Vec<String> {
        sqlx::query_scalar("SELECT event_types FROM events_fts WHERE event_id = ?")
            .bind(event_id)
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn every_seeded_slug_is_made_from_its_name() {
        let pool = database(None).await;

        let rows = sqlx::query(r#"SELECT "list", "slug", "name" FROM choice_values"#)
            .fetch_all(&pool)
            .await
            .unwrap();

        check!(rows.len() > 70);
        for row in rows {
            let (list, slug, name): (String, String, String) = (row.get(0), row.get(1), row.get(2));
            check!(slug == crate::slug::slugify(&name), "{list} {name}");
        }

        // The value the CSM finder narrows by is seeded where it looks.
        let (list, slug) = ACTS_OF_SCIENCE_CONNECTED;
        let signature: i64 = sqlx::query_scalar(
            r#"SELECT count(*) FROM choice_values WHERE "list" = ? AND "slug" = ? AND "orrery_id" = 459"#,
        )
        .bind(list)
        .bind(slug)
        .fetch_one(&pool)
        .await
        .unwrap();
        check!(signature == 1);
    }

    #[tokio::test]
    async fn sql_knows_the_lists_of_event_types_as_rust_does() {
        let pool = database(None).await;

        let lists: Vec<ChoiceList> =
            sqlx::query_scalar("SELECT list FROM event_type_lists ORDER BY rank")
                .fetch_all(&pool)
                .await
                .unwrap();
        check!(lists == ChoiceList::EVENT_TYPES);
        check!(OrreryList::EventType.lists() == ChoiceList::EVENT_TYPES);
    }

    #[test]
    fn each_list_is_copied_from_the_orrery_list_that_holds_its_facet() {
        for list in ChoiceList::ALL {
            let from = list.orrery_list();
            check!(from.lists().contains(list));
            check!(from.list_for(list.orrery_facet()) == Some(*list));
        }
        check!(OrreryList::EventType.list_for("").is_none());
        check!(OrreryList::Topics.list_for("venue").is_none());
        check!(OrreryList::from_name("Event type") == Some(OrreryList::EventType));
    }

    #[tokio::test]
    async fn the_choices_held_before_are_picked_after() {
        let pool = database(Some(CHOICE_LISTS)).await;

        add_event(&pool, "E1", "Bugs at dusk", Some("BioBlitz")).await;
        add_event(&pool, "E2", "Ask a scientist", Some("TalkOrScienceCafe")).await;
        add_event(&pool, "E3", "Stars at the branch", Some("Library")).await;
        add_event(&pool, "E4", "Tots and bugs", Some("KidFriendly")).await;
        for statement in [
            "INSERT INTO project_topics VALUES ('P1', 'Birds'), ('P1', 'Not a topic')",
            "INSERT INTO project_activities VALUES ('P1', 'At home')",
            "INSERT INTO project_audiences VALUES ('P1', 'Adults'), ('P1', 'Families')",
            "INSERT INTO misc VALUES ('event-sync-bookmark-id', 12), ('project-sync-bookmark-id', 34)",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        migrate(&pool, |version| version >= CHOICE_LISTS).await;

        check!(picks(&pool, "E1").await == vec![pick("EventFormat", "bioblitz")]);
        check!(picks(&pool, "E2").await == vec![pick("EventFormat", "talk-science-cafe")]);
        check!(picks(&pool, "E3").await == vec![pick("EventVenue", "library")]);
        // Kid-friendly is an audience now, which the event sync brings.
        check!(picks(&pool, "E4").await == vec![]);
        check!(
            picks(&pool, "P1").await
                == vec![
                    pick("Activity", "at-home"),
                    pick("Audience", "adults"),
                    pick("Audience", "families"),
                    pick("Topic", "birds"),
                ]
        );
        check!(indexed(&pool, "E1").await == vec!["BioBlitz".to_string()]);
        check!(indexed(&pool, "E3").await == vec!["library".to_string()]);

        // The event sync starts again from the beginning; the project sync,
        // which lost nothing, does not.
        let bookmarks: Vec<String> = sqlx::query_scalar("SELECT key FROM misc ORDER BY key")
            .fetch_all(&pool)
            .await
            .unwrap();
        check!(bookmarks == vec!["project-sync-bookmark-id".to_string()]);
    }

    #[tokio::test]
    async fn the_event_types_split_by_the_question_each_answers() {
        let pool = database(Some(SPLIT_EVENT_TYPES)).await;
        sqlx::query(
            "INSERT INTO choice_assignments VALUES ('E1', 'EventType', 'acts-of-science-connected'),
                 ('E1', 'EventType', 'library'), ('E1', 'EventType', 'kid-friendly'),
                 ('E1', 'EventType', 'never-was'), ('E1', 'Topic', 'birds')",
        )
        .execute(&pool)
        .await
        .unwrap();

        migrate(&pool, |version| version >= SPLIT_EVENT_TYPES).await;

        check!(
            picks(&pool, "E1").await
                == vec![
                    pick("EventProgram", "acts-of-science-connected"),
                    pick("EventVenue", "library"),
                    pick("Topic", "birds"),
                ]
        );

        let lists: Vec<(String, i64)> = sqlx::query(
            r#"SELECT "list", count(*) FROM choice_values GROUP BY "list" ORDER BY "list""#,
        )
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
        check!(
            lists
                == vec![
                    ("Activity".to_string(), 15),
                    ("Audience".to_string(), 8),
                    ("EventFormat".to_string(), 14),
                    ("EventProgram".to_string(), 6),
                    ("EventVenue".to_string(), 2),
                    ("Topic".to_string(), 29),
                ]
        );

        // Each list keeps its order.
        let formats: Vec<String> = ChoiceValue::all_from(&pool, ChoiceList::EventFormat)
            .await
            .unwrap()
            .into_iter()
            .map(|value| value.slug)
            .take(3)
            .collect();
        check!(formats == ["bioblitz", "citizen-forum", "conference"]);
    }

    #[tokio::test]
    async fn an_event_is_indexed_once_under_its_types_as_they_change() {
        let pool = database(None).await;

        add_event(&pool, "E1", "Bugs at dusk", None).await;
        check!(indexed(&pool, "E1").await == vec![String::new()]);

        sqlx::query(
            "INSERT INTO choice_assignments VALUES ('E1', 'EventVenue', 'library'),
                 ('E1', 'EventFormat', 'online'), ('E1', 'EventFormat', 'bioblitz'),
                 ('E1', 'Topic', 'birds')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Formats, then programs, then venues, each in list order.
        check!(indexed(&pool, "E1").await == vec!["BioBlitz Online library".to_string()]);

        // Saved again, as EventAll saves, with INSERT OR REPLACE.
        add_event(&pool, "E1", "Bugs at dusk", None).await;
        check!(indexed(&pool, "E1").await == vec!["BioBlitz Online library".to_string()]);

        sqlx::query(r#"UPDATE choice_values SET "name" = 'Bioblitz!' WHERE "list" = 'EventFormat' AND "slug" = 'bioblitz'"#)
            .execute(&pool)
            .await
            .unwrap();
        check!(indexed(&pool, "E1").await == vec!["Bioblitz! Online library".to_string()]);

        sqlx::query(
            r#"DELETE FROM choice_assignments WHERE "target" = 'E1' AND "value" = 'online'"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        check!(indexed(&pool, "E1").await == vec!["Bioblitz! library".to_string()]);

        sqlx::query(
            r#"DELETE FROM choice_values WHERE "list" = 'EventVenue' AND "slug" = 'library'"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        check!(indexed(&pool, "E1").await == vec!["Bioblitz!".to_string()]);
    }

    fn choice(id: i64, name: &str, list: ChoiceList) -> OrreryChoice {
        OrreryChoice {
            id,
            name: name.to_string(),
            list,
        }
    }

    async fn values(pool: &SqlitePool, list: ChoiceList) -> Vec<(String, String, Option<i64>)> {
        ChoiceValue::all_from(pool, list)
            .await
            .unwrap()
            .into_iter()
            .map(|value| (value.slug, value.name, value.orrery_id))
            .collect()
    }

    fn value(slug: &str, name: &str, id: Option<i64>) -> (String, String, Option<i64>) {
        (slug.to_string(), name.to_string(), id)
    }

    /// A list of a few values, two of them orrery's, one made here, and the
    /// projects and events that pick them.
    async fn audience(pool: &SqlitePool) {
        for statement in [
            r#"DELETE FROM choice_values WHERE "list" = 'Audience'"#,
            r#"INSERT INTO choice_values VALUES
                 ('Audience', 'adults', 'Adults', 0, 116),
                 ('Audience', 'families', 'Families', 1, 117),
                 ('Audience', 'robots', 'Robots', 2, NULL)"#,
            r#"INSERT INTO choice_assignments VALUES
                 ('P1', 'Audience', 'adults'), ('P1', 'Audience', 'families'), ('P2', 'Audience', 'robots')"#,
        ] {
            sqlx::query(statement).execute(pool).await.unwrap();
        }
    }

    fn audience_copy(values: &[(i64, &str)]) -> ChoiceListCopy {
        ChoiceListCopy::new(
            OrreryList::Audience,
            values
                .iter()
                .map(|(id, name)| choice(*id, name, ChoiceList::Audience))
                .collect(),
        )
    }

    #[tokio::test]
    async fn the_copy_takes_orrerys_names_and_order_by_id() {
        let pool = database(None).await;
        audience(&pool).await;

        // Orrery swaps the two names and reorders.
        audience_copy(&[(117, "Adults"), (116, "Families")])
            .save_into(&pool)
            .await
            .unwrap();

        check!(
            values(&pool, ChoiceList::Audience).await
                == vec![
                    value("families", "Adults", Some(117)),
                    value("adults", "Families", Some(116)),
                    value("robots", "Robots", None),
                ]
        );

        // The slugs stay, so what picked them still does.
        check!(
            picks(&pool, "P1").await
                == vec![pick("Audience", "adults"), pick("Audience", "families")]
        );
    }

    #[tokio::test]
    async fn the_copy_adds_what_orrery_adds_and_deletes_what_it_deletes() {
        let pool = database(None).await;
        audience(&pool).await;

        audience_copy(&[
            (120, "Teachers"),
            (116, "Adults"),
            // Its name's slug is taken by a value made here.
            (121, "Robots"),
        ])
        .save_into(&pool)
        .await
        .unwrap();

        check!(
            values(&pool, ChoiceList::Audience).await
                == vec![
                    value("teachers", "Teachers", Some(120)),
                    value("adults", "Adults", Some(116)),
                    value("robots", "Robots", Some(121)),
                ]
        );

        // Families went, and with it P1's pick of it. The value made here,
        // named as orrery's new one is, became it.
        check!(picks(&pool, "P1").await == vec![pick("Audience", "adults")]);
        check!(picks(&pool, "P2").await == vec![pick("Audience", "robots")]);
    }

    #[tokio::test]
    async fn a_value_orrery_renames_onto_one_made_here_takes_its_picks() {
        let pool = database(None).await;
        audience(&pool).await;

        audience_copy(&[(116, "Adults"), (117, "robots")])
            .save_into(&pool)
            .await
            .unwrap();

        check!(
            values(&pool, ChoiceList::Audience).await
                == vec![
                    value("adults", "Adults", Some(116)),
                    value("families", "robots", Some(117)),
                ]
        );
        check!(picks(&pool, "P2").await == vec![pick("Audience", "families")]);
    }

    #[tokio::test]
    async fn a_new_value_whose_slug_is_taken_gets_its_id() {
        let pool = database(None).await;
        audience(&pool).await;

        // Orrery has a second value whose name makes the slug of one it has
        // already, and a name with no letters at all.
        audience_copy(&[
            (116, "Adults"),
            (117, "Families"),
            (130, "Adults!"),
            (131, "???"),
        ])
        .save_into(&pool)
        .await
        .unwrap();

        check!(
            values(&pool, ChoiceList::Audience).await
                == vec![
                    value("adults", "Adults", Some(116)),
                    value("families", "Families", Some(117)),
                    value("adults-130", "Adults!", Some(130)),
                    value("value-131", "???", Some(131)),
                    value("robots", "Robots", None),
                ]
        );
    }

    /// The event types as a few values, and an event that picks them.
    async fn event_types(pool: &SqlitePool) {
        for statement in [
            r#"DELETE FROM choice_values WHERE "list" IN ('EventFormat', 'EventProgram', 'EventVenue')"#,
            r#"INSERT INTO choice_values VALUES
                 ('EventFormat', 'bioblitz', 'BioBlitz', 0, 187),
                 ('EventFormat', 'library', 'Library talk', 1, NULL),
                 ('EventProgram', 'ambassador', 'Ambassador', 0, 424),
                 ('EventVenue', 'library', 'library', 0, 406)"#,
            r#"INSERT INTO choice_assignments VALUES
                 ('E1', 'EventFormat', 'bioblitz'), ('E1', 'EventVenue', 'library'),
                 ('E2', 'EventFormat', 'library')"#,
        ] {
            sqlx::query(statement).execute(pool).await.unwrap();
        }
    }

    #[tokio::test]
    async fn a_value_orrery_moves_to_another_question_moves_with_its_picks() {
        let pool = database(None).await;
        event_types(&pool).await;

        // BioBlitz becomes a program, and library a format, whose list has
        // the slug "library" already.
        ChoiceListCopy::new(
            OrreryList::EventType,
            vec![
                choice(406, "library", ChoiceList::EventFormat),
                choice(424, "Ambassador", ChoiceList::EventProgram),
                choice(187, "BioBlitz", ChoiceList::EventProgram),
            ],
        )
        .save_into(&pool)
        .await
        .unwrap();

        check!(
            values(&pool, ChoiceList::EventFormat).await
                == vec![
                    value("library-406", "library", Some(406)),
                    value("library", "Library talk", None),
                ]
        );
        check!(
            values(&pool, ChoiceList::EventProgram).await
                == vec![
                    value("ambassador", "Ambassador", Some(424)),
                    value("bioblitz", "BioBlitz", Some(187)),
                ]
        );
        check!(values(&pool, ChoiceList::EventVenue).await == vec![]);

        check!(
            picks(&pool, "E1").await
                == vec![
                    pick("EventFormat", "library-406"),
                    pick("EventProgram", "bioblitz"),
                ]
        );
        check!(picks(&pool, "E2").await == vec![pick("EventFormat", "library")]);
        check!(indexed_names(&pool, "E1").await == "library BioBlitz");
    }

    /// What `event_type_names` gives for the event, as events_fts indexes it.
    async fn indexed_names(pool: &SqlitePool, event_id: &str) -> String {
        sqlx::query_scalar("SELECT names FROM events_fts_event_types WHERE event_id = ?")
            .bind(event_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn the_copy_reads_back_as_saved() {
        let pool = database(None).await;
        event_types(&pool).await;

        // Given in orrery's order, which mixes the lists.
        let copy = ChoiceListCopy::new(
            OrreryList::EventType,
            vec![
                choice(424, "Ambassador", ChoiceList::EventProgram),
                choice(187, "BioBlitz", ChoiceList::EventFormat),
                choice(406, "library", ChoiceList::EventVenue),
            ],
        );
        copy.save_into(&pool).await.unwrap();

        // Which is how the sync tells it has nothing to send.
        check!(
            ChoiceListCopy::read_from(&pool, OrreryList::EventType)
                .await
                .unwrap()
                == copy
        );
    }

    #[test]
    fn a_copy_leaves_out_a_value_that_belongs_in_no_list_of_it() {
        let copy = ChoiceListCopy::new(
            OrreryList::Audience,
            vec![
                choice(116, "Adults", ChoiceList::Audience),
                choice(9, "Birds", ChoiceList::Topic),
            ],
        );
        check!(copy == audience_copy(&[(116, "Adults")]));
    }

    #[test]
    fn a_partner_names_a_value_by_orrery_id_or_exact_name() {
        let adults = ChoiceValue {
            list: ChoiceList::Audience,
            slug: "adults".to_string(),
            name: "Adults".to_string(),
            position: 0,
            orrery_id: Some(116),
        };
        let robots = ChoiceValue {
            slug: "robots".to_string(),
            name: "Robots".to_string(),
            orrery_id: None,
            ..adults.clone()
        };
        let values = [adults, robots];
        let named = |given: serde_json::Value| {
            ChoiceValue::partner_named(&values, &given).map(|value| value.slug.as_str())
        };

        check!(named(serde_json::json!(116)) == Some("adults"));
        check!(named(serde_json::json!("Adults")) == Some("adults"));
        check!(named(serde_json::json!("adults")) == None);
        check!(named(serde_json::json!("116")) == None);
        check!(named(serde_json::json!(1)) == None);
        check!(named(serde_json::json!("Robots")) == Some("robots"));

        // A string is one value even with a comma in it; a list drops what
        // names nothing.
        check!(
            ChoiceValue::partner_all_named(&values, &serde_json::json!("Adults, Robots"))
                .is_empty()
        );
        check!(
            ChoiceValue::partner_all_named(
                &values,
                &serde_json::json!([116, "Robots", "Pirates", 116])
            ) == BTreeSet::from(["adults".to_string(), "robots".to_string()])
        );
    }
}
