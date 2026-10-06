//! Which referral source a new account came from.
//!
//! **Shared file.** This module is byte-identical in orrery
//! (`src/staff/referrals/sources.rs`) and hydra
//! (`common/src/referrals/sources.rs`), as are its siblings `chart.rs`,
//! `report.rs` and `page.rs`, and the report kit they draw on
//! (`report_kit/`), so both sites draw the same report from the same rules.
//! Change both copies together.
//!
//! An account carries the attribution of its owner's first page load: the
//! `Referer` of that request, its UTM tags, and its `origin`, captured by each
//! site's first-touch middleware. [`classify`] turns those into a [`Source`], the
//! row label of the report's per-month table, and every source belongs to a
//! [`Group`], the series it is stacked under in the chart.
//!
//! UTM tags are consulted before the referrer, because a tag is an explicit
//! statement about where a link was placed (a paid ad, a newsletter) where
//! the referrer only names the site the click came from: a Facebook ad and a
//! Facebook post send the same `Referer`.

use std::fmt;

/// Where an account's owner came from, at the granularity the table reports.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Source {
    GoogleSearch,
    OtherSearch,
    CourseSites,
    GirlScouts,
    Nasa,
    CoCoRaHS,
    FacebookInstagramAds,
    FacebookInstagramOrganic,
    LinkedInAds,
    LinkedInOrganic,
    Verizon,
    Newsletter,
    Idealist,
    AiAssistants,
    Webmail,
    SciStarterWordPress,
    /// Created through the account API by the named project.
    Api(String),
    /// Arrived without a referrer, but with this `utm_source`.
    Tagged(String),
    /// Any other referring site, by host.
    Site(String),
}

/// The chart series a source is stacked under. The order is the stacking
/// order, bottom to top, and the categorical color order.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Group {
    GoogleSearch,
    CourseSites,
    GirlScouts,
    Nasa,
    CoCoRaHS,
    OtherSearch,
    FacebookInstagram,
    LinkedIn,
    Other,
}

impl Group {
    pub const ALL: [Group; 9] = [
        Group::GoogleSearch,
        Group::CourseSites,
        Group::GirlScouts,
        Group::Nasa,
        Group::CoCoRaHS,
        Group::OtherSearch,
        Group::FacebookInstagram,
        Group::LinkedIn,
        Group::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::GoogleSearch => "Google search",
            Group::CourseSites => "University/school course sites",
            Group::GirlScouts => "Girl Scouts",
            Group::Nasa => "NASA",
            Group::CoCoRaHS => "CoCoRaHS",
            Group::OtherSearch => "Other search engines",
            Group::FacebookInstagram => "Facebook/Instagram",
            Group::LinkedIn => "LinkedIn",
            Group::Other => "All other sources",
        }
    }

    /// The series color: the validated categorical palette in slot order,
    /// and a neutral for the fold-in "other" series rather than a ninth hue.
    pub fn color(self) -> &'static str {
        match self {
            Group::GoogleSearch => "#2a78d6",
            Group::CourseSites => "#eb6834",
            Group::GirlScouts => "#1baf7a",
            Group::Nasa => "#eda100",
            Group::CoCoRaHS => "#e87ba4",
            Group::OtherSearch => "#008300",
            Group::FacebookInstagram => "#4a3aa7",
            Group::LinkedIn => "#e34948",
            Group::Other => "#b4b2aa",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

impl Source {
    pub fn group(&self) -> Group {
        match self {
            Source::GoogleSearch => Group::GoogleSearch,
            Source::OtherSearch => Group::OtherSearch,
            Source::CourseSites => Group::CourseSites,
            Source::GirlScouts => Group::GirlScouts,
            Source::Nasa => Group::Nasa,
            Source::CoCoRaHS => Group::CoCoRaHS,
            Source::FacebookInstagramAds | Source::FacebookInstagramOrganic => {
                Group::FacebookInstagram
            }
            Source::LinkedInAds | Source::LinkedInOrganic => Group::LinkedIn,
            Source::Verizon
            | Source::Newsletter
            | Source::Idealist
            | Source::AiAssistants
            | Source::Webmail
            | Source::SciStarterWordPress
            | Source::Api(_)
            | Source::Tagged(_)
            | Source::Site(_) => Group::Other,
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::GoogleSearch => f.write_str("Google search"),
            Source::OtherSearch => f.write_str("Other search engines"),
            Source::CourseSites => f.write_str("University/school course sites"),
            Source::GirlScouts => f.write_str("Girl Scouts"),
            Source::Nasa => f.write_str("NASA"),
            Source::CoCoRaHS => f.write_str("CoCoRaHS"),
            Source::FacebookInstagramAds => f.write_str("Facebook/Instagram ads"),
            Source::FacebookInstagramOrganic => f.write_str("Facebook/Instagram (organic)"),
            Source::LinkedInAds => f.write_str("LinkedIn ads"),
            Source::LinkedInOrganic => f.write_str("LinkedIn (organic)"),
            Source::Verizon => f.write_str("Verizon volunteer program"),
            Source::Newsletter => f.write_str("SciStarter newsletter"),
            Source::Idealist => f.write_str("Idealist"),
            Source::AiAssistants => f.write_str("AI assistants (ChatGPT etc.)"),
            Source::Webmail => f.write_str("Webmail links"),
            Source::SciStarterWordPress => f.write_str("SciStarter WordPress/blog"),
            Source::Api(project) => write!(f, "API: {project}"),
            Source::Tagged(utm_source) => write!(f, "Tagged “{utm_source}”"),
            Source::Site(host) => f.write_str(host),
        }
    }
}

/// The attribution columns of one account.
#[derive(Debug, Clone, Copy)]
pub struct Attribution<'a> {
    pub referer: &'a str,
    pub origin: &'a str,
    pub utm_source: &'a str,
    pub utm_medium: &'a str,
}

/// The host of a referrer URL, lowercased, or None if it isn't a URL. App
/// referrers (`android-app://com.google.android.gm/`) name the app's package
/// in the host position, which is what the rules match on.
fn referer_host(referer: &str) -> Option<String> {
    let (scheme, rest) = referer.split_once("://")?;

    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    {
        return None;
    }

    let end = rest.find(['/', ':', '?', '#']).unwrap_or(rest.len());
    let host = rest[..end].to_ascii_lowercase();

    (!host.is_empty()).then_some(host)
}

fn is_paid(medium: &str) -> bool {
    matches!(
        medium,
        "paid" | "cpc" | "ppc" | "paid_social" | "paidsocial"
    )
}

fn is_linkedin_tag(utm_source: &str) -> bool {
    utm_source.starts_with("linkedin") || utm_source == "li"
}

/// Hosts that belong to a learning platform: a course page linking out to
/// SciStarter as a class assignment. Many institutions run their own
/// instance, so this matches the platform rather than a list of schools.
fn is_course_site(host: &str) -> bool {
    const PLATFORMS: &[&str] = &[
        "instructure",
        "moodle",
        "desire2learn",
        "d2l",
        "brightspace",
        "blackboard",
        "schoology",
        "itslearning",
        "canvas.",
        "learn.",
        "mycourses",
        "classroom.google",
    ];

    host.starts_with("bb.") || PLATFORMS.iter().any(|platform| host.contains(platform))
}

fn is_search_engine(host: &str) -> bool {
    const ENGINES: &[&str] = &[
        "bing.com",
        "yahoo.com",
        "duckduckgo.com",
        "brave.com",
        "ecosia.org",
        "yandex.",
        "baidu.com",
    ];

    ENGINES.iter().any(|engine| host.contains(engine))
}

fn is_google_search(host: &str) -> bool {
    let host = host.strip_prefix("www.").unwrap_or(host);
    host.starts_with("google.") || host == "com.google.android.googlequicksearchbox"
}

fn is_ai_assistant(host: &str) -> bool {
    matches!(
        host,
        "chatgpt.com"
            | "gemini.google.com"
            | "perplexity.ai"
            | "www.perplexity.ai"
            | "copilot.microsoft.com"
            | "copilot.com"
            | "claude.ai"
    )
}

fn is_facebook_or_instagram(host: &str) -> bool {
    host == "facebook.com" || host.ends_with(".facebook.com") || host.contains("instagram.com")
}

fn is_linkedin(host: &str) -> bool {
    host == "linkedin.com"
        || host.ends_with(".linkedin.com")
        || host == "lnkd.in"
        || host == "com.linkedin.android"
}

fn is_webmail(host: &str) -> bool {
    matches!(
        host,
        "mail.google.com"
            | "outlook.live.com"
            | "outlook.office.com"
            | "outlook.office365.com"
            | "com.google.android.gm"
    )
}

fn is_domain(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// The source an account came from, or None when there is nothing to tell:
/// no referrer and no tags, or a referrer on our own site.
pub fn classify(attribution: Attribution<'_>) -> Option<Source> {
    let utm_source = attribution.utm_source.trim().to_lowercase();
    let utm_medium = attribution.utm_medium.trim().to_lowercase();

    if attribution.origin == "API" {
        let project = attribution.referer.trim();
        return Some(Source::Api(if project.is_empty() {
            String::from("no project")
        } else {
            project.to_string()
        }));
    }

    if utm_medium == "newsletter" {
        return Some(Source::Newsletter);
    }

    if utm_source == "idealist" {
        return Some(Source::Idealist);
    }

    if matches!(utm_source.as_str(), "fb" | "ig" | "facebook" | "instagram") && is_paid(&utm_medium)
    {
        return Some(Source::FacebookInstagramAds);
    }

    if is_linkedin_tag(&utm_source) {
        return Some(if is_paid(&utm_medium) {
            Source::LinkedInAds
        } else {
            Source::LinkedInOrganic
        });
    }

    if matches!(
        utm_source.as_str(),
        "chatgpt.com" | "perplexity" | "gemini" | "copilot.com"
    ) {
        return Some(Source::AiAssistants);
    }

    let referer = attribution.referer.trim();

    let Some(host) = referer_host(referer) else {
        if !referer.is_empty() {
            return Some(Source::Site(referer.to_string()));
        }

        return (!utm_source.is_empty()).then_some(Source::Tagged(utm_source));
    };

    let source = if is_ai_assistant(&host) {
        Source::AiAssistants
    } else if is_google_search(&host) {
        Source::GoogleSearch
    } else if is_search_engine(&host) {
        Source::OtherSearch
    } else if is_course_site(&host) {
        Source::CourseSites
    } else if is_domain(&host, "girlscouts.org") {
        Source::GirlScouts
    } else if host.contains("verizon") {
        Source::Verizon
    } else if is_domain(&host, "nasa.gov") {
        Source::Nasa
    } else if is_domain(&host, "cocorahs.org") {
        Source::CoCoRaHS
    } else if is_facebook_or_instagram(&host) {
        Source::FacebookInstagramOrganic
    } else if is_linkedin(&host) {
        Source::LinkedInOrganic
    } else if is_webmail(&host) {
        Source::Webmail
    } else if host == "pages.scistarter.org" || host == "blog.scistarter.org" {
        Source::SciStarterWordPress
    } else if is_domain(&host, "scistarter.org") || is_domain(&host, "scistarter.com") {
        // A click from one of our own pages says nothing about how the
        // visitor reached the site, unless it was tagged.
        return (!utm_source.is_empty()).then_some(Source::Tagged(utm_source));
    } else {
        Source::Site(host.strip_prefix("www.").unwrap_or(&host).to_string())
    };

    Some(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from(referer: &str) -> Option<Source> {
        classify(Attribution {
            referer,
            origin: "/",
            utm_source: "",
            utm_medium: "",
        })
    }

    fn tagged(referer: &str, utm_source: &str, utm_medium: &str) -> Option<Source> {
        classify(Attribution {
            referer,
            origin: "/",
            utm_source,
            utm_medium,
        })
    }

    #[test]
    fn search_engines() {
        assert_eq!(from("https://www.google.com/"), Some(Source::GoogleSearch));
        assert_eq!(
            from("https://www.google.co.uk/search?q=x"),
            Some(Source::GoogleSearch)
        );
        assert_eq!(
            from("android-app://com.google.android.googlequicksearchbox/"),
            Some(Source::GoogleSearch)
        );
        assert_eq!(from("https://www.bing.com/"), Some(Source::OtherSearch));
        assert_eq!(from("https://duckduckgo.com/"), Some(Source::OtherSearch));
    }

    #[test]
    fn google_properties_that_are_not_search_are_their_own_sites() {
        assert_eq!(
            from("https://gemini.google.com/app"),
            Some(Source::AiAssistants)
        );
        assert_eq!(
            from("https://mail.google.com/mail/u/0/"),
            Some(Source::Webmail)
        );
        assert_eq!(
            from("https://classroom.google.com/c/1"),
            Some(Source::CourseSites)
        );
        assert_eq!(
            from("https://docs.google.com/document/d/1"),
            Some(Source::Site(String::from("docs.google.com")))
        );
    }

    #[test]
    fn learning_platforms_are_course_sites() {
        for referer in [
            "https://csub.instructure.com/courses/1",
            "https://moodle.adelphi.edu/",
            "https://georgiasouthern.desire2learn.com/",
            "https://canvas.asu.edu/",
            "https://bb.nsuok.edu/",
            "https://learn.snhu.edu/",
            "https://mycourses.siu.edu/",
        ] {
            assert_eq!(from(referer), Some(Source::CourseSites), "{referer}");
        }
    }

    #[test]
    fn partner_sites() {
        assert_eq!(
            from("https://www.girlscouts.org/"),
            Some(Source::GirlScouts)
        );
        assert_eq!(from("https://my.girlscouts.org/"), Some(Source::GirlScouts));
        assert_eq!(from("https://science.nasa.gov/x"), Some(Source::Nasa));
        assert_eq!(from("https://www.cocorahs.org/"), Some(Source::CoCoRaHS));
        assert_eq!(from("https://verizon.webex.com/"), Some(Source::Verizon));
        assert_eq!(
            from("https://volunteer.verizon.com/"),
            Some(Source::Verizon)
        );
    }

    #[test]
    fn a_lookalike_domain_is_not_the_partner() {
        assert_eq!(
            from("https://notnasa.gov/"),
            Some(Source::Site(String::from("notnasa.gov")))
        );
    }

    #[test]
    fn social_referrers_are_organic() {
        assert_eq!(
            from("https://m.facebook.com/"),
            Some(Source::FacebookInstagramOrganic)
        );
        assert_eq!(
            from("https://l.instagram.com/"),
            Some(Source::FacebookInstagramOrganic)
        );
        assert_eq!(
            from("https://www.linkedin.com/"),
            Some(Source::LinkedInOrganic)
        );
        assert_eq!(from("https://lnkd.in/abc"), Some(Source::LinkedInOrganic));
        assert_eq!(
            from("android-app://com.linkedin.android/"),
            Some(Source::LinkedInOrganic)
        );
    }

    #[test]
    fn paid_tags_mark_ads_whatever_the_referrer() {
        assert_eq!(
            tagged("https://m.facebook.com/", "fb", "paid"),
            Some(Source::FacebookInstagramAds)
        );
        assert_eq!(tagged("", "ig", "paid"), Some(Source::FacebookInstagramAds));
        assert_eq!(
            tagged("https://www.linkedin.com/", "linkedin", "paid"),
            Some(Source::LinkedInAds)
        );
        assert_eq!(tagged("", "LinkedIn", "CPC"), Some(Source::LinkedInAds));
    }

    #[test]
    fn an_unpaid_linkedin_tag_is_organic() {
        assert_eq!(
            tagged("", "linkedin", "social"),
            Some(Source::LinkedInOrganic)
        );
    }

    #[test]
    fn a_facebook_tag_without_paid_falls_back_to_the_referrer() {
        assert_eq!(
            tagged("https://m.facebook.com/", "facebook", "social"),
            Some(Source::FacebookInstagramOrganic)
        );
    }

    #[test]
    fn campaign_tags() {
        assert_eq!(
            tagged("https://scistarter.org/", "featured", "newsletter"),
            Some(Source::Newsletter)
        );
        assert_eq!(tagged("", "idealist", "referral"), Some(Source::Idealist));
        assert_eq!(tagged("", "chatgpt.com", ""), Some(Source::AiAssistants));
        assert_eq!(tagged("", "copilot.com", ""), Some(Source::AiAssistants));
    }

    #[test]
    fn our_own_pages_say_nothing_unless_tagged() {
        assert_eq!(from("https://scistarter.org/"), None);
        assert_eq!(from("https://www.scistarter.org/finder"), None);
        assert_eq!(from("https://scistarter.com/"), None);
        assert_eq!(
            tagged("https://scistarter.org/", "sciencenearme", "web"),
            Some(Source::Tagged(String::from("sciencenearme")))
        );
    }

    #[test]
    fn our_wordpress_sites_are_a_source() {
        assert_eq!(
            from("https://pages.scistarter.org/x"),
            Some(Source::SciStarterWordPress)
        );
        assert_eq!(
            from("https://blog.scistarter.org/"),
            Some(Source::SciStarterWordPress)
        );
    }

    #[test]
    fn nothing_recorded_is_no_source() {
        assert_eq!(from(""), None);
        assert_eq!(tagged("", "", ""), None);
    }

    #[test]
    fn a_tag_alone_is_a_source() {
        assert_eq!(
            tagged("", "sciencenearme", "web"),
            Some(Source::Tagged(String::from("sciencenearme")))
        );
    }

    #[test]
    fn api_accounts_are_attributed_to_the_calling_project() {
        let source = classify(Attribution {
            referer: "iseatree",
            origin: "API",
            utm_source: "",
            utm_medium: "",
        });
        assert_eq!(source, Some(Source::Api(String::from("iseatree"))));

        let without = classify(Attribution {
            referer: "",
            origin: "API",
            utm_source: "",
            utm_medium: "",
        });
        assert_eq!(without, Some(Source::Api(String::from("no project"))));
    }

    #[test]
    fn other_sites_are_named_by_host_without_www() {
        assert_eq!(
            from("https://www.VolunteerMatch.org:443/search?x=1"),
            Some(Source::Site(String::from("volunteermatch.org")))
        );
    }

    #[test]
    fn a_referrer_that_is_not_a_url_is_kept_as_written() {
        assert_eq!(
            from("iseatree"),
            Some(Source::Site(String::from("iseatree")))
        );
        assert_eq!(referer_host("://nothing"), None);
        assert_eq!(referer_host("https://"), None);
    }

    #[test]
    fn every_source_belongs_to_a_group() {
        assert_eq!(Source::LinkedInAds.group(), Group::LinkedIn);
        assert_eq!(Source::LinkedInOrganic.group(), Group::LinkedIn);
        assert_eq!(
            Source::FacebookInstagramAds.group(),
            Group::FacebookInstagram
        );
        assert_eq!(Source::Newsletter.group(), Group::Other);
        assert_eq!(Source::Site(String::from("x.org")).group(), Group::Other);
    }

    #[test]
    fn group_indexes_follow_the_stacking_order() {
        for (i, group) in Group::ALL.into_iter().enumerate() {
            assert_eq!(group.index(), i);
        }
    }
}
