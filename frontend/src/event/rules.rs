//! What SciStarter asks of an event, in the words orrery's event form settled
//! on. The add form and the event page's inline editors make these same
//! checks, so a rule and its wording live here once.
//!
//! A value that was saved before and is left as it was is never refused for
//! breaking a rule made since (an older event's longer name, say): each check
//! that orrery's form makes only of a changed value takes the `stored` value,
//! which is `None` for a new event.

use std::borrow::Cow;

use jiff::{Timestamp, Zoned, civil, tz::TimeZone};

use crate::{
    contact::Unreachable,
    event::{EventInstance, location::VirtualJoin},
    project::location::{LocationMode, missing_place},
    upload::storage::untracked_file_id,
    web_address::is_web_address,
};

/// The most characters (code points) an event's name may have, as orrery's
/// `SciStarter.EVENT_NAME_LIMIT`.
pub const NAME_LIMIT: usize = 100;

/// The most characters (code points) the group an event is limited to may
/// have, as orrery's `SciStarter.EVENT_LIMITED_TO_LIMIT`.
pub const LIMITED_TO_LIMIT: usize = 140;

/// The control a refusal is about. Each form names its controls its own way,
/// and maps these onto them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Description,
    ImageLink,
    Url,
    /// Whether a virtual event is joined at a link or by instructions.
    VirtualJoin,
    HowToJoin,
    TicketUrl,
    LimitedTo,
    LocationMode,
    Address,
    LocationDescription,
    TicketingDate,
    TicketingTimezone,
    ContactName,
    ContactEmail,
    ContactUrl,
    /// A part of a date/time row.
    Date(RowPart),
}

/// The parts of a date/time row a refusal can be about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowPart {
    BeginDate,
    BeginTime,
    EndDate,
    Timezone,
}

impl RowPart {
    /// The name of the row's control for this part.
    pub fn name(self) -> &'static str {
        match self {
            RowPart::BeginDate => "begin_date",
            RowPart::BeginTime => "begin_time",
            RowPart::EndDate => "end_date",
            RowPart::Timezone => "timezone",
        }
    }
}

/// Why an event can't be saved as it stands, and which control to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub field: Field,
    /// The other controls the problem is about, any of which would fix it:
    /// marked with `field`, though the message is told once, at `field`.
    pub also: Vec<Field>,
    pub message: Cow<'static, str>,
}

impl Refusal {
    fn new(field: Field, message: impl Into<Cow<'static, str>>) -> Self {
        Refusal {
            field,
            also: Vec::new(),
            message: message.into(),
        }
    }

    /// Implicate `field` in the problem as well.
    fn also(mut self, field: Field) -> Self {
        self.also.push(field);
        self
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

pub type Checked<T = ()> = Result<T, Refusal>;

/// Whether `value` differs from what was saved, as orrery's form compares
/// them: a new event's every value is a change.
fn changed(value: &str, stored: Option<&str>) -> bool {
    stored.is_none_or(|stored| stored.trim() != value.trim())
}

pub fn name(name: &str, stored: Option<&str>) -> Checked {
    let name = name.trim();
    if name.is_empty() {
        return Err(Refusal::new(
            Field::Name,
            "Please enter a name for your event.",
        ));
    }
    // Counted in characters, as the field counts them, not UTF-16 units.
    if changed(name, stored) && name.chars().count() > NAME_LIMIT {
        return Err(Refusal::new(
            Field::Name,
            format!("Please shorten your event's name to {NAME_LIMIT} characters or fewer."),
        ));
    }
    Ok(())
}

pub fn description(description: &str) -> Checked {
    if description.trim().is_empty() {
        return Err(Refusal::new(
            Field::Description,
            "Please add a description of your event.",
        ));
    }
    Ok(())
}

/// An uploaded image's address is the site's own; only a typed link needs
/// checking.
pub fn image_link(image: &str, stored: Option<&str>) -> Checked {
    let image = image.trim();
    if image.is_empty() || untracked_file_id(image).is_some() || !changed(image, stored) {
        return Ok(());
    }
    if !is_web_address(image) {
        return Err(Refusal::new(
            Field::ImageLink,
            "Please check the image link; it should be a whole web address, such as https://example.org/photo.jpg.",
        ));
    }
    Ok(())
}

/// How people join an event, for a form that asks it: an event held
/// somewhere keeps the link it was given, while a virtual one is joined at
/// its link or, by Facebook's "Other", by instructions in place of one. The
/// link the event is saved with, which is none for a virtual event joined by
/// instructions, or why it can't be saved.
pub fn join<'a>(
    mode: LocationMode,
    choice: Option<VirtualJoin>,
    url: &'a str,
    how_to_join: &str,
) -> Checked<&'a str> {
    if mode != LocationMode::Online {
        return Ok(url);
    }
    match choice {
        None => Err(Refusal::new(
            Field::VirtualJoin,
            "Please choose how people join the virtual event.",
        )),
        Some(VirtualJoin::Link) if url.trim().is_empty() => {
            Err(Refusal::new(Field::Url, JOIN_LINK))
        }
        Some(VirtualJoin::Link) => Ok(url),
        Some(VirtualJoin::Other) if how_to_join.trim().is_empty() => {
            Err(Refusal::new(Field::HowToJoin, JOIN_INSTRUCTIONS))
        }
        Some(VirtualJoin::Other) => Ok(""),
    }
}

/// A virtual event keeps a way to join, its link or its instructions, when
/// one of them is changed on its own (`edited`, [`Field::Url`] or
/// [`Field::HowToJoin`]), where the form's choice between them isn't asked.
pub fn way_to_join(mode: LocationMode, url: &str, how_to_join: &str, edited: Field) -> Checked {
    if mode == LocationMode::Online && url.trim().is_empty() && how_to_join.trim().is_empty() {
        let message = if edited == Field::HowToJoin {
            JOIN_INSTRUCTIONS
        } else {
            JOIN_LINK
        };
        return Err(Refusal::new(edited, message));
    }
    Ok(())
}

const JOIN_LINK: &str = "Please provide a link where people can join the virtual event.";
const JOIN_INSTRUCTIONS: &str = "Please add instructions on how people join the virtual event.";

/// The event's link, when it has one.
pub fn link(url: &str, stored: Option<&str>) -> Checked {
    let url = url.trim();
    if !url.is_empty() && changed(url, stored) && !is_web_address(url) {
        return Err(Refusal::new(
            Field::Url,
            "Please check the event's link; it should be a whole web address, such as https://example.org/event.",
        ));
    }
    Ok(())
}

pub fn ticket_link(url: &str, stored: Option<&str>) -> Checked {
    let url = url.trim();
    if !url.is_empty() && changed(url, stored) && !is_web_address(url) {
        return Err(Refusal::new(
            Field::TicketUrl,
            "Please check the ticket / registration link; it should be a whole web address, such as https://example.org/tickets.",
        ));
    }
    Ok(())
}

/// Who may attend, when the event is `limited` to a group.
pub fn limited_to(group: &str, limited: bool) -> Checked {
    // An event open to everyone has no group, whatever was left typed.
    let group = if limited { group.trim() } else { "" };
    if limited && group.is_empty() {
        return Err(Refusal::new(
            Field::LimitedTo,
            "Please say which group can attend, such as \u{201c}Library cardholders.\u{201d}",
        ));
    }
    // Counted in characters, as the field counts them.
    if group.chars().count() > LIMITED_TO_LIMIT {
        return Err(Refusal::new(
            Field::LimitedTo,
            format!("Please say which group can attend in {LIMITED_TO_LIMIT} characters or fewer."),
        ));
    }
    Ok(())
}

pub fn mode(mode: Option<LocationMode>) -> Checked<LocationMode> {
    mode.ok_or_else(|| {
        Refusal::new(
            Field::LocationMode,
            "Please choose where the event takes place.",
        )
    })
}

/// An event held at an address has one, or at least a city. Words left in the
/// place search (`place_query`) were never picked or used as the address, so
/// the refusal then says to do one or the other.
pub fn place(mode: LocationMode, address: &str, city: &str, place_query: &str) -> Checked {
    if mode != LocationMode::InPerson {
        return Ok(());
    }
    match missing_place(address, city, place_query, "event") {
        Some(message) => Err(Refusal::new(Field::Address, message)),
        None => Ok(()),
    }
}

/// A region is described in words as well as drawn.
pub fn location_note(mode: LocationMode, note: &str) -> Checked {
    if mode == LocationMode::Region && note.trim().is_empty() {
        return Err(Refusal::new(
            Field::LocationDescription,
            "Please describe the area the event covers.",
        ));
    }
    Ok(())
}

/// SciStarter's own requirement, not hydra's: staff need someone to ask about
/// the event, and a way to reach them, by email or through a contact form.
/// `stored` is the email and contact form link saved before.
pub fn contact(name: &str, email: &str, url: &str, stored: Option<(&str, &str)>) -> Checked {
    if name.trim().is_empty() {
        return Err(Refusal::new(
            Field::ContactName,
            "Please add the contact person's name.",
        ));
    }
    let (stored_email, stored_url) = stored.unzip();
    // A value left as it was counts toward reaching the contact without
    // being checked again.
    let kept =
        |value: &str, stored: Option<&str>| !changed(value, stored) && !value.trim().is_empty();
    let reachable_as_stored = kept(email, stored_email) || kept(url, stored_url);
    let email = if changed(email, stored_email) {
        email.trim()
    } else {
        ""
    };
    let url = if changed(url, stored_url) {
        url.trim()
    } else {
        ""
    };
    match Unreachable::check(email, url) {
        Ok(()) => Ok(()),
        Err(Unreachable::Neither) if reachable_as_stored => Ok(()),
        // Neither is given: either field would do, so both are at fault.
        Err(Unreachable::Neither) => Err(Refusal::new(
            Field::ContactEmail,
            Unreachable::Neither.message(),
        )
        .also(Field::ContactUrl)),
        Err(refusal @ Unreachable::Email) => {
            Err(Refusal::new(Field::ContactEmail, refusal.message()))
        }
        Err(refusal @ Unreachable::Url) => Err(Refusal::new(Field::ContactUrl, refusal.message())),
    }
}

/// When tickets or registration open, on `timezone`'s clocks at `time` (or
/// midnight) on `date`; `None` when no date is given.
pub fn ticketing(date: &str, time: &str, timezone: &str) -> Checked<Option<Timestamp>> {
    let date = date.trim();
    if date.is_empty() {
        return Ok(None);
    }
    let timezone = timezone.trim();
    if timezone.is_empty() {
        return Err(Refusal::new(
            Field::TicketingTimezone,
            "Please choose a time zone for the ticketing date.",
        ));
    }
    let time = match time.trim() {
        "" => "00:00",
        time => time,
    };
    let begins = parse_datetime(date, time, timezone)
        .map_err(|message| Refusal::new(Field::TicketingDate, message))?;
    Ok(Some(begins.timestamp()))
}

/// One date/time row as a form submits it: a start, an end when one is
/// given, and the time zone whose clocks both are read on.
#[derive(Debug, Clone, Copy)]
pub struct DateRow<'a> {
    pub begin_date: &'a str,
    pub begin_time: &'a str,
    /// The end date and time, when the row has an end.
    pub end: Option<(&'a str, &'a str)>,
    pub timezone: &'a str,
}

/// A date's start and, when it has one, its end, kept on the clocks they were
/// given on, so a repetition steps by those clocks across a daylight-saving
/// change.
#[derive(Debug, Clone, PartialEq)]
pub struct ZonedInstance {
    pub begin: Zoned,
    pub end: Option<Zoned>,
}

impl ZonedInstance {
    /// The instance as it is stored.
    pub fn stored(&self) -> EventInstance {
        stored_instance(&self.begin, self.end.as_ref())
    }
}

/// An instance as it is stored: one with no end ends as it begins, a
/// point-in-time event.
pub fn stored_instance(begin: &Zoned, end: Option<&Zoned>) -> EventInstance {
    EventInstance {
        begin: begin.timestamp().into(),
        end: end.unwrap_or(begin).timestamp().into(),
    }
}

/// What a date/time row comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum RowReading {
    /// Nothing was entered in it.
    Blank,
    /// It's a saved date, left as it was saved, which is kept just so.
    Kept,
    Dated(ZonedInstance),
}

/// Read one date/time row. `stored` is the date the row showed when it was
/// opened, for a saved date: left as it was, the date is kept as saved, even
/// one that has passed, or that ends before it starts.
pub fn date_row(
    row: DateRow<'_>,
    stored: Option<&EventInstance>,
    now: &Zoned,
) -> Result<RowReading, Refusal> {
    let refuse = |part, message: &'static str| Err(Refusal::new(Field::Date(part), message));
    let begin_date = row.begin_date.trim();
    let begin_time = row.begin_time.trim();

    if begin_date.is_empty() && begin_time.is_empty() {
        return Ok(RowReading::Blank);
    }
    if begin_date.is_empty() {
        return refuse(
            RowPart::BeginDate,
            "Each date/time needs both a date and a start time.",
        );
    }
    if begin_time.is_empty() {
        return refuse(
            RowPart::BeginTime,
            "Each date/time needs both a date and a start time.",
        );
    }
    let timezone = row.timezone.trim();
    if timezone.is_empty() {
        return refuse(
            RowPart::Timezone,
            "Please choose a time zone for each date/time.",
        );
    }

    let begin = parse_datetime(begin_date, begin_time, timezone)
        .map_err(|message| Refusal::new(Field::Date(RowPart::BeginDate), message))?;
    let end = match row.end {
        Some((end_date, end_time))
            if !end_date.trim().is_empty() && !end_time.trim().is_empty() =>
        {
            Some(
                parse_datetime(end_date.trim(), end_time.trim(), timezone)
                    .map_err(|message| Refusal::new(Field::Date(RowPart::EndDate), message))?,
            )
        }
        _ => None,
    };
    let instance = ZonedInstance { begin, end };

    if stored.is_some_and(|stored| same_minutes(&instance.stored(), stored)) {
        return Ok(RowReading::Kept);
    }

    if starts_before_today(&instance.begin, now) {
        return refuse(
            RowPart::BeginDate,
            "An event can't start on a date that has already passed.",
        );
    }
    if instance
        .end
        .as_ref()
        .is_some_and(|end| *end < instance.begin)
    {
        return refuse(
            RowPart::EndDate,
            "An event's end time can't be before its start time.",
        );
    }

    Ok(RowReading::Dated(instance))
}

/// Whether `begin` falls on a day before `now`'s, both read in `begin`'s own
/// zone: "today" is the event's today, wherever the person is. Earlier the
/// same day is allowed, so an event can be listed while it's under way.
fn starts_before_today(begin: &Zoned, now: &Zoned) -> bool {
    begin.date() < now.with_time_zone(begin.time_zone().clone()).date()
}

/// Refused when an event would be left with no dates.
pub fn some_date(dated: bool) -> Checked {
    if !dated {
        return Err(Refusal::new(
            Field::Date(RowPart::BeginDate),
            "Please add at least one date and start time.",
        ));
    }
    Ok(())
}

/// Whether two instances start and end in the same minutes, as a form shows
/// them: a saved second or two isn't a change the form could make.
fn same_minutes(a: &EventInstance, b: &EventInstance) -> bool {
    let minutes = |time: &str| {
        time.parse::<Timestamp>()
            .ok()
            .map(|t| t.as_second().div_euclid(60))
    };
    minutes(&a.begin) == minutes(&b.begin) && minutes(&a.end) == minutes(&b.end)
}

/// The moment `date` and `time` (`HH:MM` or `HH:MM:SS`) name on `timezone`'s
/// clocks.
pub fn parse_datetime(date: &str, time: &str, timezone: &str) -> Result<Zoned, String> {
    let tz = TimeZone::get(timezone)
        .map_err(|_| format!("Unrecognized time zone \u{201c}{timezone}\u{201d}."))?;

    let time = if time.len() == 5 {
        format!("{time}:00")
    } else {
        time.to_string()
    };

    let datetime: civil::DateTime = format!("{date}T{time}")
        .parse()
        .map_err(|_| "Please enter a valid date and time.".to_string())?;

    datetime
        .to_zoned(tz)
        .map_err(|e| format!("That date and time isn't valid in the chosen time zone: {e}"))
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    fn now() -> Zoned {
        "2027-03-10T12:00:00-05:00[America/New_York]"
            .parse()
            .unwrap()
    }

    fn row<'a>(date: &'a str, time: &'a str, end: Option<(&'a str, &'a str)>) -> DateRow<'a> {
        DateRow {
            begin_date: date,
            begin_time: time,
            end,
            timezone: "America/New_York",
        }
    }

    fn stored(begin: &str, end: &str) -> EventInstance {
        EventInstance {
            begin: TimeString::try_from(begin.to_string()).unwrap(),
            end: TimeString::try_from(end.to_string()).unwrap(),
        }
    }

    use common::timestring::TimeString;

    #[test]
    fn a_new_date_cant_have_passed() {
        let refusal = date_row(row("2027-03-09", "10:00", None), None, &now()).unwrap_err();
        check!(refusal.field == Field::Date(RowPart::BeginDate));
        check!(refusal.message == "An event can't start on a date that has already passed.");
    }

    #[test]
    fn a_saved_date_left_alone_is_kept_though_it_has_passed() {
        let saved = stored("2027-03-01T15:00:00Z", "2027-03-01T15:00:00Z");
        let reading = date_row(row("2027-03-01", "10:00", None), Some(&saved), &now()).unwrap();
        check!(reading == RowReading::Kept);
    }

    #[test]
    fn a_saved_date_moved_is_checked_as_a_new_one() {
        let saved = stored("2027-03-01T15:00:00Z", "2027-03-01T15:00:00Z");
        let refusal = date_row(row("2027-03-02", "10:00", None), Some(&saved), &now()).unwrap_err();
        check!(refusal.message == "An event can't start on a date that has already passed.");
    }

    #[test]
    fn a_saved_date_that_ends_before_it_starts_is_kept_as_it_was() {
        let saved = stored("2027-03-12T16:00:00Z", "2027-03-12T15:00:00Z");
        let reading = date_row(
            row("2027-03-12", "11:00", Some(("2027-03-12", "10:00"))),
            Some(&saved),
            &now(),
        )
        .unwrap();
        check!(reading == RowReading::Kept);
    }

    #[test]
    fn an_end_cant_come_before_the_start() {
        let refusal = date_row(
            row("2027-03-12", "11:00", Some(("2027-03-12", "10:00"))),
            None,
            &now(),
        )
        .unwrap_err();
        check!(refusal.field == Field::Date(RowPart::EndDate));
    }

    #[test]
    fn a_date_without_an_end_ends_as_it_begins() {
        let RowReading::Dated(instance) =
            date_row(row("2027-03-12", "11:00", None), None, &now()).unwrap()
        else {
            panic!("not dated");
        };
        let stored = instance.stored();
        check!(stored.begin.to_string() == "2027-03-12T16:00:00Z");
        check!(stored.end == stored.begin);
    }

    #[test]
    fn a_date_needs_its_start_time_and_zone() {
        let refusal = date_row(row("2027-03-12", "", None), None, &now()).unwrap_err();
        check!(refusal.field == Field::Date(RowPart::BeginTime));
        let mut zoneless = row("2027-03-12", "11:00", None);
        zoneless.timezone = "";
        let refusal = date_row(zoneless, None, &now()).unwrap_err();
        check!(refusal.field == Field::Date(RowPart::Timezone));
    }

    #[test]
    fn past_start_dates_are_judged_in_the_events_zone() {
        // 02:00 UTC on 2026-09-21 is still the evening of the 20th in Chicago.
        let now: Zoned = "2026-09-21T02:00:00+00:00[UTC]".parse().unwrap();

        let chicago_evening = parse_datetime("2026-09-20", "21:00", "America/Chicago").unwrap();
        check!(!starts_before_today(&chicago_evening, &now));

        let chicago_yesterday = parse_datetime("2026-09-19", "21:00", "America/Chicago").unwrap();
        check!(starts_before_today(&chicago_yesterday, &now));

        let tokyo_same_instant = parse_datetime("2026-09-20", "21:00", "Asia/Tokyo").unwrap();
        check!(starts_before_today(&tokyo_same_instant, &now));
    }

    #[test]
    fn an_older_long_name_is_kept_until_it_changes() {
        let long = "x".repeat(NAME_LIMIT + 1);
        check!(name(&long, Some(&long)).is_ok());
        check!(name(&long, None).is_err());
        check!(name("", Some("")).is_err());
    }

    #[test]
    fn a_virtual_event_says_how_people_join() {
        let refusal = join(LocationMode::Online, None, "https://example.org", "").unwrap_err();
        check!(refusal.field == Field::VirtualJoin);
        check!(refusal.message == "Please choose how people join the virtual event.");
    }

    #[test]
    fn a_virtual_event_joined_at_a_link_has_one() {
        let refusal = join(
            LocationMode::Online,
            Some(VirtualJoin::Link),
            " ",
            "Dial in",
        )
        .unwrap_err();
        check!(refusal.field == Field::Url);
        check!(refusal.message == "Please provide a link where people can join the virtual event.");
        check!(
            join(
                LocationMode::Online,
                Some(VirtualJoin::Link),
                "https://example.org",
                ""
            ) == Ok("https://example.org")
        );
    }

    #[test]
    fn a_virtual_event_joined_otherwise_has_instructions_and_no_link() {
        let refusal = join(
            LocationMode::Online,
            Some(VirtualJoin::Other),
            "https://example.org",
            " ",
        )
        .unwrap_err();
        check!(refusal.field == Field::HowToJoin);
        check!(refusal.message == "Please add instructions on how people join the virtual event.");
        check!(
            join(
                LocationMode::Online,
                Some(VirtualJoin::Other),
                "https://example.org",
                "Dial in"
            ) == Ok("")
        );
    }

    #[test]
    fn an_event_held_somewhere_keeps_its_link_unasked() {
        check!(join(LocationMode::InPerson, None, "", "") == Ok(""));
        check!(
            join(
                LocationMode::Region,
                Some(VirtualJoin::Other),
                "https://example.org",
                ""
            ) == Ok("https://example.org")
        );
    }

    #[test]
    fn a_virtual_event_keeps_a_link_or_instructions() {
        let refusal = way_to_join(LocationMode::Online, " ", "", Field::Url).unwrap_err();
        check!(refusal.field == Field::Url);
        check!(refusal.message == "Please provide a link where people can join the virtual event.");

        let refusal = way_to_join(LocationMode::Online, "", " ", Field::HowToJoin).unwrap_err();
        check!(refusal.field == Field::HowToJoin);
        check!(refusal.message == "Please add instructions on how people join the virtual event.");

        check!(way_to_join(LocationMode::Online, "", "Dial in", Field::Url).is_ok());
        check!(
            way_to_join(
                LocationMode::Online,
                "https://example.org",
                "",
                Field::HowToJoin
            )
            .is_ok()
        );
        check!(way_to_join(LocationMode::InPerson, "", "", Field::Url).is_ok());
    }

    #[test]
    fn a_saved_link_left_alone_is_not_checked_again() {
        check!(link("example.org", Some("example.org")).is_ok());
        check!(link("example.org", Some("")).is_err());
        check!(ticket_link("tickets", Some("tickets")).is_ok());
        check!(ticket_link("tickets", None).is_err());
    }

    #[test]
    fn a_saved_email_left_alone_still_reaches_the_contact() {
        // An older contact's email, saved before it was checked.
        check!(contact("Ada", "ada at example", "", Some(("ada at example", ""))).is_ok());
        let refusal = contact("Ada", "ada at example", "", None).unwrap_err();
        check!(refusal.field == Field::ContactEmail);
        let refusal = contact("Ada", "", "", Some(("ada@example.org", ""))).unwrap_err();
        check!(refusal.field == Field::ContactEmail);
        // Either would reach them, so both are at fault.
        check!(refusal.also == [Field::ContactUrl]);
        let refusal = contact("Ada", "ada at example", "", None).unwrap_err();
        check!(refusal.also.is_empty());
        check!(contact("", "ada@example.org", "", None).unwrap_err().field == Field::ContactName);
    }
}
