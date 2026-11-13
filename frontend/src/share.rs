//! Sharing a page to social networks.
//!
//! Every share control on the site is an ordinary link to the network's own
//! share dialog. Nothing is loaded from a network until the visitor follows
//! one of those links, so a page that offers sharing sets no third-party
//! cookies and tells no network who looked at it.
//!
//! The URL builders here are the only place a share link is spelled out;
//! [`ShareLinks`] is the standard row of controls built from them, and
//! surfaces with their own markup (the exchange widget) call the builders
//! directly.

use sailfish::TemplateOnce;

/// Facebook's share dialog for `url`. Facebook takes no suggested text.
pub fn facebook_url(url: &str) -> String {
    format!(
        "https://www.facebook.com/sharer/sharer.php?u={}",
        urlencoding::encode(url)
    )
}

/// X's compose dialog, prefilled with `text` and `url`.
pub fn twitter_url(url: &str, text: &str) -> String {
    format!(
        "https://twitter.com/intent/tweet?url={}&text={}",
        urlencoding::encode(url),
        urlencoding::encode(text)
    )
}

/// LinkedIn's share dialog for `url`. LinkedIn takes no suggested text.
pub fn linkedin_url(url: &str) -> String {
    format!(
        "https://www.linkedin.com/sharing/share-offsite/?url={}",
        urlencoding::encode(url)
    )
}

/// A `mailto:` link that opens the visitor's own mail program with a subject
/// and body filled in.
pub fn email_url(subject: &str, body: &str) -> String {
    format!(
        "mailto:?subject={}&body={}",
        urlencoding::encode(subject),
        urlencoding::encode(body)
    )
}

/// An email share's prefilled message.
pub struct EmailShare<'a> {
    pub subject: &'a str,
    /// The whole message, link included: a `mailto:` link has nowhere else
    /// to put one.
    pub body: &'a str,
}

/// The standard row of share controls.
#[derive(TemplateOnce)]
#[template(path = "component/share_links.stpl.html")]
pub struct ShareLinks<'a> {
    /// The absolute URL being shared.
    pub url: &'a str,
    /// The suggested post, for the networks that accept one, and the caption
    /// copied for Instagram.
    pub text: &'a str,
    /// Absolute URL of an image to hand to Instagram, or empty for none.
    pub image: &'a str,
    /// The entity being shared, whose share clicks are counted. `None` for a
    /// page that is not an entity.
    pub target_id: Option<&'a str>,
    /// Offers an email share when present.
    pub email: Option<EmailShare<'a>>,
    /// Centres the row, for a dialog; otherwise it starts at the leading edge
    /// like the text around it.
    pub centered: bool,
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[test]
    fn shared_values_are_encoded_into_the_query() {
        let url = "https://scistarter.org/projects/a&b?c=d";

        check!(
            facebook_url(url)
                == "https://www.facebook.com/sharer/sharer.php?u=https%3A%2F%2Fscistarter.org%2Fprojects%2Fa%26b%3Fc%3Dd"
        );
        check!(
            twitter_url(url, "Me & you")
                == "https://twitter.com/intent/tweet?url=https%3A%2F%2Fscistarter.org%2Fprojects%2Fa%26b%3Fc%3Dd&text=Me%20%26%20you"
        );
        check!(
            email_url("Hi", "Line one\n\nLine two")
                == "mailto:?subject=Hi&body=Line%20one%0A%0ALine%20two"
        );
    }
}
