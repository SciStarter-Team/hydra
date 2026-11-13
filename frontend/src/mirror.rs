//! Third-party files this site serves itself.
//!
//! A page that loads a script or stylesheet from a CDN tells the CDN who is
//! looking at it. So the libraries this site's pages use are downloaded by
//! `static/vendor.sh` and served from here instead: each one at
//! `static/mirror/<host>/<path>`, which is to say at its original URL with
//! `https://` replaced by `/-static/mirror/`. The version is part of every
//! path, so a mirrored copy is the exact file the original URL names.
//!
//! Templates refer to their mirrored files directly. Markup that arrives from
//! elsewhere --- an embedded WordPress page --- names the originals, and
//! [`local_url`] finds the copy for each.

use url::Url;

/// The local address of the mirrored copy of `url`, if this site has one.
///
/// A query string is not part of the lookup: WordPress appends `?ver=…` to
/// the files it loads, and the version that matters is the one in the path.
pub fn local_url(url: &str) -> Option<String> {
    let parsed = match url.strip_prefix("//") {
        Some(rest) => Url::parse(&format!("https://{rest}")),
        None => Url::parse(url),
    }
    .ok()?;

    if !matches!(parsed.scheme(), "http" | "https") || parsed.port().is_some() {
        return None;
    }

    let path = format!("mirror/{}{}", parsed.host_str()?, parsed.path());

    crate::Static::get(&path).map(|_| format!("/-static/{path}"))
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[test]
    fn mirrored_files_are_found_by_their_original_urls() {
        let local = "/-static/mirror/code.jquery.com/ui/1.14.2/jquery-ui.min.js";

        check!(
            local_url("https://code.jquery.com/ui/1.14.2/jquery-ui.min.js").as_deref()
                == Some(local)
        );
        check!(
            local_url("https://code.jquery.com/ui/1.14.2/jquery-ui.min.js?ver=1.14.2").as_deref()
                == Some(local)
        );
        check!(local_url("//code.jquery.com/ui/1.14.2/jquery-ui.min.js").as_deref() == Some(local));
    }

    #[test]
    fn anything_else_is_left_alone() {
        check!(local_url("https://code.jquery.com/ui/1.13.0/jquery-ui.min.js") == None);
        check!(local_url("https://code.jquery.com:8443/ui/1.14.2/jquery-ui.min.js") == None);
        check!(local_url("ftp://code.jquery.com/ui/1.14.2/jquery-ui.min.js") == None);
        check!(local_url("/-static/jquery.js") == None);
    }
}
