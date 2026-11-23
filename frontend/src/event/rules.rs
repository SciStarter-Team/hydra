//! What SciStarter asks of an event, in the words orrery's event form settled
//! on. The add form and the event page's inline editors make these same
//! checks, so a rule and its wording live here once.
//!
//! A value that was saved before and is left as it was is never refused for
//! breaking a rule made since (an older event's longer name, say): each check
//! that orrery's form makes only of a changed value takes the `stored` value,
//! which is `None` for a new event.

use std::{borrow::Cow, collections::BTreeSet};

use jiff::{Timestamp, Zoned, civil, tz::TimeZone};

use crate::{
    choice::ChoiceList,
    component::{floating_field::FormError, image_field::ImageUrlInput},
    contact::Unreachable,
    event::{
        EventInstance,
        admission::{Admission, EventAdmission},
        attendance::{Attendance, EventAttendance},
        registration::Registration,
    },
    project::location::{LocationMode, PlaceGiven, PlaceOf, missing_place},
    upload::storage::untracked_file_id,
    web_address::is_web_address,
};

/// The most characters (code points) an event's name may have, as orrery's
/// `SciStarter.EVENT_NAME_LIMIT`.
pub const NAME_LIMIT: usize = 100;

/// The most characters (code points) the audience an event is meant for may
/// have, as orrery's `SciStarter.EVENT_LIMITED_TO_LIMIT`.
pub const LIMITED_TO_LIMIT: usize = 140;

/// The control a refusal is about. The event forms name their controls alike
/// ([`Field::control`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Description,
    /// The cover image, uploaded or linked.
    ImageLink,
    Url,
    HowToJoin,
    TicketUrl,
    /// The answer to "Admission".
    Admission,
    /// The price, or the suggested donation.
    Cost,
    LimitedTo,
    LocationMode,
    Address,
    LocationDescription,
    TicketingDate,
    TicketingTimezone,
    ContactName,
    ContactEmail,
    ContactUrl,
    Topics,
    /// The larger event the event is part of.
    PartOf,
    /// A part of a date/time row.
    Date(RowPart),
}

/// The parts of a date/time row a refusal can be about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowPart {
    BeginDate,
    BeginTime,
    EndDate,
    EndTime,
    Timezone,
    /// The date a repeating row repeats through (the add form's alone).
    RepeatUntil,
}

impl RowPart {
    /// The name of the row's control for this part.
    pub fn name(self) -> &'static str {
        match self {
            RowPart::BeginDate => "begin_date",
            RowPart::BeginTime => "begin_time",
            RowPart::EndDate => "end_date",
            RowPart::EndTime => "end_time",
            RowPart::Timezone => "timezone",
            RowPart::RepeatUntil => "repeat_until",
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
    pub(crate) fn new(field: Field, message: impl Into<Cow<'static, str>>) -> Self {
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

impl Refusal {
    /// The refusal as the event forms show it: told at the control `field`
    /// names, marking the others too. The add event form and the event
    /// page's editors name their controls alike; a date's are its date/time
    /// row `row`'s.
    pub fn into_form_error(self, row: usize) -> FormError {
        self.also.iter().fold(
            FormError::at(self.field.control(row), self.message),
            |error, field| error.also(field.control(row)),
        )
    }
}

impl Field {
    /// The name of the event forms' control for this, a date's in date/time
    /// row `row`.
    pub fn control(self, row: usize) -> String {
        match self {
            Field::Name => "name".to_string(),
            Field::Description => "description".to_string(),
            Field::ImageLink => ImageUrlInput::NAME.to_string(),
            Field::Url => "url".to_string(),
            Field::HowToJoin => "how_to_join".to_string(),
            Field::TicketUrl => "ticket_url".to_string(),
            Field::Admission => "admission".to_string(),
            Field::Cost => "cost".to_string(),
            Field::LimitedTo => "limited_to".to_string(),
            Field::LocationMode => "location_mode".to_string(),
            Field::Address => "address".to_string(),
            Field::LocationDescription => "location_description".to_string(),
            Field::TicketingDate => "ticketing_date".to_string(),
            Field::TicketingTimezone => "ticketing_timezone".to_string(),
            Field::ContactName => "contact_name".to_string(),
            Field::ContactEmail => "contact_email".to_string(),
            Field::ContactUrl => "contact_url".to_string(),
            Field::Topics => "topics".to_string(),
            Field::PartOf => "part_of".to_string(),
            Field::Date(part) => format!("instances[{row}][{}]", part.name()),
        }
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

/// Every new event has a cover image, uploaded or linked; an older event
/// saved without one may be saved so until it's given one. An uploaded
/// image's address is the site's own; only a typed link needs checking.
pub fn image(image: &str, stored: Option<&str>) -> Checked {
    let image = image.trim();
    if !changed(image, stored) || untracked_file_id(image).is_some() {
        return Ok(());
    }
    if image.is_empty() {
        return Err(Refusal::new(
            Field::ImageLink,
            "Please add a cover image for your event.",
        ));
    }
    if !is_web_address(image) {
        return Err(Refusal::new(
            Field::ImageLink,
            "Please check the image link; it should be a whole web address, such as https://example.org/photo.jpg.",
        ));
    }
    Ok(())
}

/// A virtual event says how people join it: under "How to participate", or
/// at its webpage, or where people register. The refusal is told under the
/// instructions, and marks the links too.
pub fn way_to_join(mode: LocationMode, how_to_join: &str, url: &str, ticket_url: &str) -> Checked {
    let nothing = |value: &str| value.trim().is_empty();
    if mode == LocationMode::Online && nothing(how_to_join) && nothing(url) && nothing(ticket_url) {
        return Err(Refusal::new(
            Field::HowToJoin,
            "Please explain how people join the virtual event, or add its webpage or registration link.",
        )
        .also(Field::Url)
        .also(Field::TicketUrl));
    }
    Ok(())
}

/// The event's webpage, when it has one.
pub fn link(url: &str, stored: Option<&str>) -> Checked {
    let url = url.trim();
    if !url.is_empty() && changed(url, stored) && !is_web_address(url) {
        return Err(Refusal::new(
            Field::Url,
            "Please check the event's webpage; it should be a whole web address, such as https://example.org/event.",
        ));
    }
    Ok(())
}

/// Where people register or get tickets: asked of every event, and required
/// of one that requires registration.
pub fn ticket_link(url: &str, registration: Option<Registration>, stored: Option<&str>) -> Checked {
    let url = url.trim();
    if registration == Some(Registration::Required) && url.is_empty() {
        return Err(Refusal::new(
            Field::TicketUrl,
            "Please add the link where people register.",
        ));
    }
    if !url.is_empty() && changed(url, stored) && !is_web_address(url) {
        return Err(Refusal::new(
            Field::TicketUrl,
            "Please check the registration or ticket link; it should be a whole web address, such as https://example.org/register.",
        ));
    }
    Ok(())
}

/// Whether an event may pick `picked` distinct topics: at least one, and no
/// more than [`crate::event::TOPIC_LIMIT`]. Topics are asked of every new
/// event, as of every project, since the finder's topic filter is how most
/// people narrow the listings. An event saved without any (`stored`, how
/// many it was saved with) may be saved so until it's given some, as an
/// unanswered admission may.
pub fn topics(picked: usize, stored: Option<usize>) -> Checked {
    if let Some(refusal) = crate::event::refuse_picks(ChoiceList::Topic, picked) {
        return Err(Refusal::new(Field::Topics, refusal));
    }
    if picked == 0 && stored != Some(0) {
        return Err(Refusal::new(
            Field::Topics,
            "Please choose at least one topic.",
        ));
    }
    Ok(())
}

/// What attending costs, as it is saved: the answer to "Admission", with the
/// price or the suggested donation when the answer asks for one (a free
/// event saves none), or why it can't be saved. Admission is asked of every
/// new event. An event saved before it was asked (`stored`) may be saved
/// unanswered until it's answered, keeping the cost it was saved with; so
/// may a paid one saved without a price.
pub fn admission(
    answer: Option<Admission>,
    cost: &str,
    stored: Option<&EventAdmission>,
) -> Checked<EventAdmission> {
    let answer_changed = stored.is_none_or(|stored| stored.admission != answer);
    let cost_changed = changed(cost, stored.map(|stored| stored.cost.as_str()));
    let cost = if answer.is_none_or(Admission::asks_cost) {
        cost.trim()
    } else {
        ""
    };

    if answer.is_none() && answer_changed {
        return Err(Refusal::new(
            Field::Admission,
            "Please choose whether admission is free, paid, or a suggested donation.",
        ));
    }
    if answer == Some(Admission::Paid) && cost.is_empty() && (answer_changed || cost_changed) {
        return Err(Refusal::new(
            Field::Cost,
            "Please give the admission price.",
        ));
    }
    Ok(EventAdmission {
        admission: answer,
        cost: cost.to_string(),
    })
}

/// Who the event is meant for, when it is `limited` to a specific audience.
pub fn limited_to(group: &str, limited: bool) -> Checked {
    // An event for everyone names no audience, whatever was left typed.
    let group = if limited { group.trim() } else { "" };
    if limited && group.is_empty() {
        return Err(Refusal::new(
            Field::LimitedTo,
            "Please say who the event is for, such as \u{201c}Educators\u{201d} or \u{201c}Girl Scouts.\u{201d}",
        ));
    }
    // Counted in characters, as the field counts them.
    if group.chars().count() > LIMITED_TO_LIMIT {
        return Err(Refusal::new(
            Field::LimitedTo,
            format!("Please say who the event is for in {LIMITED_TO_LIMIT} characters or fewer."),
        ));
    }
    Ok(())
}

/// Who can participate, as it is saved: everyone, or the specific audience
/// named and the `ages` picked for it, or why it can't be saved. An event for
/// everyone names no audience, whatever was left typed, and keeps the ages
/// it was saved with (`stored`), which it isn't asked; a new one has none.
pub fn attendance(
    answer: Attendance,
    group: &str,
    ages: BTreeSet<String>,
    stored: Option<&EventAttendance>,
) -> Checked<EventAttendance> {
    let limited = answer == Attendance::Limited;
    limited_to(group, limited)?;
    Ok(if limited {
        EventAttendance {
            limited_to: group.trim().to_string(),
            ages,
        }
    } else {
        EventAttendance {
            limited_to: String::new(),
            ages: stored.map(|stored| stored.ages.clone()).unwrap_or_default(),
        }
    })
}

pub fn mode(mode: Option<LocationMode>) -> Checked<LocationMode> {
    mode.ok_or_else(|| {
        Refusal::new(
            Field::LocationMode,
            "Please choose where the event takes place.",
        )
    })
}

/// An event held at an address has one, or at least a city
/// ([`missing_place`] says what to do instead).
pub fn place(mode: LocationMode, place: PlaceGiven) -> Checked {
    if mode != LocationMode::InPerson {
        return Ok(());
    }
    match missing_place(place, PlaceOf::Event) {
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

/// When registration opens, on `timezone`'s clocks at `time` (or midnight)
/// on `date`: `None` when no date is given, and for an event nobody
/// registers for (`registration`), which is never asked.
pub fn registration_opens(
    registration: Option<Registration>,
    date: &str,
    time: &str,
    timezone: &str,
) -> Checked<Option<Timestamp>> {
    let date = date.trim();
    if !Registration::asks_opening(registration) || date.is_empty() {
        return Ok(None);
    }
    let timezone = timezone.trim();
    if timezone.is_empty() {
        return Err(Refusal::new(
            Field::TicketingTimezone,
            "Please choose a time zone for the registration opening date.",
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
    // An end is both a date and a time; with one of them missing, the row
    // can't be saved with the end it was meant to have, so it is refused
    // rather than saved with none. With both left blank, there's no end.
    let end = match row.end.map(|(date, time)| (date.trim(), time.trim())) {
        None | Some(("", "")) => None,
        Some((_, "")) => {
            return refuse(
                RowPart::EndTime,
                "Please choose the time the event ends, or remove the end time.",
            );
        }
        Some(("", _)) => {
            return refuse(
                RowPart::EndDate,
                "Please choose the date the event ends, or remove the end time.",
            );
        }
        Some((end_date, end_time)) => Some(
            parse_datetime(end_date, end_time, timezone)
                .map_err(|message| Refusal::new(Field::Date(RowPart::EndDate), message))?,
        ),
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
    // An end at the start would be stored as no end at all, so an end has to
    // come after the start.
    if instance
        .end
        .as_ref()
        .is_some_and(|end| *end <= instance.begin)
    {
        return Err(Refusal::new(
            Field::Date(RowPart::EndDate),
            "An event's end time must be after its start time.",
        )
        .also(Field::Date(RowPart::EndTime)));
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

    #[test]
    fn topics_are_asked_of_every_new_event_but_kept_unanswered_by_one_without() {
        check!(topics(1, None).is_ok());
        check!(topics(0, None).unwrap_err().message == "Please choose at least one topic.");
        check!(topics(0, None).unwrap_err().field == Field::Topics);
        // Taken away from an event that had some.
        check!(topics(0, Some(2)).is_err());
        // An event saved without any may stay so until it's given some.
        check!(topics(0, Some(0)).is_ok());
        check!(topics(4, Some(0)).unwrap_err().message == "Please pick no more than 3 topics.");
    }

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
        check!(refusal.also == vec![Field::Date(RowPart::EndTime)]);
        check!(refusal.message == "An event's end time must be after its start time.");
    }

    #[test]
    fn an_end_cant_be_the_start() {
        // Stored, it would read as no end at all.
        let refusal = date_row(
            row("2027-03-12", "11:00", Some(("2027-03-12", "11:00"))),
            None,
            &now(),
        )
        .unwrap_err();
        check!(refusal.field == Field::Date(RowPart::EndDate));
        check!(refusal.message == "An event's end time must be after its start time.");
    }

    #[test]
    fn a_half_given_end_is_refused_not_dropped() {
        let refusal = date_row(
            row("2027-03-12", "11:00", Some(("2027-03-12", " "))),
            None,
            &now(),
        )
        .unwrap_err();
        check!(refusal.field == Field::Date(RowPart::EndTime));
        check!(refusal.message == "Please choose the time the event ends, or remove the end time.");

        let refusal = date_row(
            row("2027-03-12", "11:00", Some(("", "12:00"))),
            None,
            &now(),
        )
        .unwrap_err();
        check!(refusal.field == Field::Date(RowPart::EndDate));
        check!(refusal.message == "Please choose the date the event ends, or remove the end time.");
    }

    #[test]
    fn a_saved_end_half_cleared_is_refused() {
        let saved = stored("2027-03-12T16:00:00Z", "2027-03-12T17:00:00Z");
        let refusal = date_row(
            row("2027-03-12", "11:00", Some(("2027-03-12", ""))),
            Some(&saved),
            &now(),
        )
        .unwrap_err();
        check!(refusal.field == Field::Date(RowPart::EndTime));
    }

    #[test]
    fn an_end_left_blank_is_no_end() {
        let RowReading::Dated(instance) =
            date_row(row("2027-03-12", "11:00", Some(("", ""))), None, &now()).unwrap()
        else {
            panic!("not dated");
        };
        check!(instance.end == None);
    }

    #[test]
    fn a_saved_date_without_an_end_left_alone_is_kept() {
        let saved = stored("2027-03-12T16:00:00Z", "2027-03-12T16:00:00Z");
        let reading = date_row(row("2027-03-12", "11:00", None), Some(&saved), &now()).unwrap();
        check!(reading == RowReading::Kept);
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
        let refusal = way_to_join(LocationMode::Online, " ", "", "").unwrap_err();
        check!(refusal.field == Field::HowToJoin);
        check!(refusal.also == vec![Field::Url, Field::TicketUrl]);
        check!(
            refusal.message
                == "Please explain how people join the virtual event, or add its webpage or registration link."
        );
    }

    #[test]
    fn instructions_a_webpage_or_a_registration_link_will_do() {
        check!(way_to_join(LocationMode::Online, "Dial in", "", "").is_ok());
        check!(way_to_join(LocationMode::Online, "", "https://example.org", "").is_ok());
        check!(way_to_join(LocationMode::Online, "", "", "https://example.org/register").is_ok());
    }

    #[test]
    fn an_event_held_somewhere_needn_t_say_how_people_join() {
        check!(way_to_join(LocationMode::InPerson, "", "", "").is_ok());
        check!(way_to_join(LocationMode::Anywhere, "", "", "").is_ok());
    }

    #[test]
    fn a_saved_link_left_alone_is_not_checked_again() {
        check!(link("example.org", Some("example.org")).is_ok());
        check!(link("example.org", Some("")).is_err());
        check!(ticket_link("tickets", None, Some("tickets")).is_ok());
        check!(ticket_link("tickets", None, None).is_err());
    }

    #[test]
    fn an_event_requiring_registration_says_where() {
        let refusal = ticket_link(" ", Some(Registration::Required), None).unwrap_err();
        check!(refusal.field == Field::TicketUrl);
        check!(refusal.message == "Please add the link where people register.");
        check!(ticket_link("", Some(Registration::Optional), None).is_ok());
        check!(ticket_link("", None, None).is_ok());
    }

    #[test]
    fn a_new_event_has_a_cover_image() {
        let refusal = image(" ", None).unwrap_err();
        check!(refusal.field == Field::ImageLink);
        check!(refusal.message == "Please add a cover image for your event.");
        check!(image("https://example.org/photo.jpg", None).is_ok());
        // An older event saved without one may stay so.
        check!(image("", Some("")).is_ok());
        // One that had an image can't lose it.
        check!(image("", Some("https://example.org/photo.jpg")).is_err());
    }

    fn saved(admission: Option<Admission>, cost: &str) -> EventAdmission {
        EventAdmission {
            admission,
            cost: cost.to_string(),
        }
    }

    #[test]
    fn a_new_event_answers_admission() {
        let refusal = admission(None, "$5", None).unwrap_err();
        check!(refusal.field == Field::Admission);
        check!(
            refusal.message
                == "Please choose whether admission is free, paid, or a suggested donation."
        );
    }

    #[test]
    fn an_older_event_may_stay_unanswered_with_its_cost() {
        let stored = saved(None, "Free with museum entry");
        check!(admission(None, "Free with museum entry", Some(&stored)) == Ok(stored.clone()));
        // Its answer, once given, can't be taken back.
        let answered = saved(Some(Admission::Free), "");
        check!(admission(None, "", Some(&answered)).is_err());
    }

    #[test]
    fn a_paid_event_gives_its_price() {
        let refusal = admission(Some(Admission::Paid), " ", None).unwrap_err();
        check!(refusal.field == Field::Cost);
        check!(refusal.message == "Please give the admission price.");
        check!(
            admission(Some(Admission::Paid), "$10", None)
                == Ok(saved(Some(Admission::Paid), "$10"))
        );
        // One saved without a price may stay so until either changes.
        let stored = saved(Some(Admission::Paid), "");
        check!(admission(Some(Admission::Paid), "", Some(&stored)).is_ok());
    }

    #[test]
    fn a_free_event_saves_no_cost_and_a_donation_may_name_none() {
        check!(
            admission(Some(Admission::Free), "$10", None) == Ok(saved(Some(Admission::Free), ""))
        );
        check!(
            admission(Some(Admission::Donation), "", None)
                == Ok(saved(Some(Admission::Donation), ""))
        );
    }

    #[test]
    fn nobody_registering_has_no_opening_date() {
        check!(
            registration_opens(Some(Registration::NotNeeded), "2027-03-12", "09:00", "")
                == Ok(None)
        );
        let refusal = registration_opens(Some(Registration::Optional), "2027-03-12", "09:00", "")
            .unwrap_err();
        check!(refusal.field == Field::TicketingTimezone);
        check!(
            registration_opens(None, "2027-03-12", "", "America/New_York")
                == Ok(Some("2027-03-12T05:00:00Z".parse().unwrap()))
        );
    }

    #[test]
    fn an_event_for_a_specific_audience_names_it() {
        let refusal = limited_to(" ", true).unwrap_err();
        check!(refusal.field == Field::LimitedTo);
        check!(
            refusal.message
                == "Please say who the event is for, such as \u{201c}Educators\u{201d} or \u{201c}Girl Scouts.\u{201d}"
        );
        check!(limited_to("Anything left typed", false).is_ok());
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
