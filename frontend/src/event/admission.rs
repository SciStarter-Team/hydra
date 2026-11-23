//! What attending an event costs, as orrery's event form asks it: whether
//! admission is free, paid or a suggested donation, and the price or the
//! donation. An event saved before the question was asked has no answer, and
//! keeps the free-text cost its organizer gave ("Free", "$5").
//!
//! The answer and the cost are one value to change: the cost means the price
//! or the donation only once the question is answered, and a free event has
//! none. So the event page edits them together.

use anyhow::Error;
use common::shared::db;
use ulid::Ulid;

/// The answer to "Admission".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Admission {
    Free,
    Paid,
    /// A suggested donation.
    Donation,
}

impl Admission {
    /// The answers, in the order the forms offer them.
    pub const ALL: [Admission; 3] = [Admission::Free, Admission::Paid, Admission::Donation];

    /// The answer as it is stored and submitted.
    pub fn as_str(self) -> &'static str {
        match self {
            Admission::Free => "free",
            Admission::Paid => "paid",
            Admission::Donation => "donation",
        }
    }

    /// The answer `value` names, or `None` for no answer or one that isn't
    /// known.
    pub fn parse(value: &str) -> Option<Admission> {
        Admission::ALL
            .into_iter()
            .find(|answer| answer.as_str() == value.trim())
    }

    /// The answer as the forms offer it.
    pub fn label(self) -> &'static str {
        match self {
            Admission::Free => "Free",
            Admission::Paid => "Paid",
            Admission::Donation => "Suggested donation",
        }
    }

    /// Whether the answer asks for the cost: the price, or the suggested
    /// donation.
    pub fn asks_cost(self) -> bool {
        matches!(self, Admission::Paid | Admission::Donation)
    }

    /// `answer` as its column stores it: empty for none.
    pub fn stored(answer: Option<Admission>) -> &'static str {
        answer.map_or("", Admission::as_str)
    }
}

/// What attending an event costs.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EventAdmission {
    /// `None` for an event that was never asked.
    pub admission: Option<Admission>,
    /// The price, or the suggested donation, for an event whose answer asks
    /// for it; the organizer's own words for one that was never asked; and
    /// nothing otherwise.
    pub cost: String,
}

impl EventAdmission {
    /// What the event page says attending costs, or `None` when it says
    /// nothing: the answer, with the price or the suggested donation, or,
    /// for an event that was never asked, the cost it gave, if any.
    pub fn text(&self) -> Option<String> {
        let cost = self.cost.trim();
        let text = match self.admission {
            Some(Admission::Free) => "Free".to_string(),
            Some(Admission::Paid) if cost.is_empty() => "Paid".to_string(),
            Some(Admission::Paid) => cost.to_string(),
            Some(Admission::Donation) if cost.is_empty() => "Suggested donation".to_string(),
            Some(Admission::Donation) => format!("Suggested donation: {cost}"),
            None if cost.is_empty() => return None,
            None => cost.to_string(),
        };
        Some(text)
    }

    /// The admission of the event `event_id`, as it is saved.
    pub async fn read(event_id: Ulid) -> Result<Self, Error> {
        let id = event_id.to_string();
        let row = sqlx::query!("SELECT admission, cost FROM events WHERE event_id = ?", id)
            .fetch_one(db())
            .await?;
        Ok(EventAdmission {
            admission: Admission::parse(&row.admission),
            cost: row.cost,
        })
    }

    /// Save this as the admission of the event `event_id`.
    pub async fn save(&self, event_id: Ulid) -> Result<(), Error> {
        let id = event_id.to_string();
        let admission = Admission::stored(self.admission);
        sqlx::query!(
            "UPDATE events SET admission = ?, cost = ? WHERE event_id = ?",
            admission,
            self.cost,
            id
        )
        .execute(db())
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    fn admission(admission: Option<Admission>, cost: &str) -> Option<String> {
        EventAdmission {
            admission,
            cost: cost.to_string(),
        }
        .text()
    }

    #[test]
    fn the_page_says_the_answer_with_its_price() {
        check!(admission(Some(Admission::Free), "") == Some("Free".to_string()));
        check!(
            admission(Some(Admission::Paid), "$10 per person")
                == Some("$10 per person".to_string())
        );
        check!(admission(Some(Admission::Paid), " ") == Some("Paid".to_string()));
        check!(
            admission(Some(Admission::Donation), "$5")
                == Some("Suggested donation: $5".to_string())
        );
        check!(admission(Some(Admission::Donation), "") == Some("Suggested donation".to_string()));
    }

    #[test]
    fn an_event_never_asked_shows_the_cost_it_gave() {
        check!(
            admission(None, "Free with museum entry") == Some("Free with museum entry".to_string())
        );
        check!(admission(None, "") == None);
    }

    #[test]
    fn answers_read_as_they_are_stored() {
        for answer in Admission::ALL {
            check!(Admission::parse(Admission::stored(Some(answer))) == Some(answer));
        }
        check!(Admission::parse("") == None);
        check!(Admission::parse("pay what you can") == None);
    }
}
