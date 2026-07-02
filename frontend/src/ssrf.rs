//! SSRF-guarded outbound HTTP.
//!
//! Some staff-facing features fetch a user-supplied URL server-side (archiving a
//! page snapshot, importing a WordPress page). Without restriction those become
//! server-side request forgery vectors: a URL like `http://169.254.169.254/…`
//! or `http://localhost:…/` would let the caller reach cloud-metadata endpoints
//! or internal-only services from the server's network position.
//!
//! [`guarded_get`] performs a GET that:
//!  - permits only the `http`/`https` schemes,
//!  - resolves the host and rejects the request if *any* resolved address is
//!    private, loopback, link-local, or otherwise not publicly routable,
//!  - pins the connection to the validated address so a second, unchecked DNS
//!    lookup cannot be substituted (DNS rebinding), and
//!  - re-runs every check on each redirect hop.

use std::net::{IpAddr, SocketAddr};

use anyhow::{Error, anyhow, bail};
use url::Url;

/// Maximum number of redirects to follow before giving up.
const MAX_REDIRECTS: usize = 10;

/// Fetch `url` with SSRF protections. See the module docs for the guarantees.
pub async fn guarded_get(url: &str) -> Result<reqwest::Response, Error> {
    let mut current = Url::parse(url)?;

    for _ in 0..=MAX_REDIRECTS {
        let host = current
            .host_str()
            .ok_or_else(|| anyhow!("URL has no host"))?
            .to_string();
        let addr = validated_addr(&current).await?;

        // Disable reqwest's own redirect following so we can validate each hop,
        // and pin the resolved host to the address we just checked so the
        // connection cannot be redirected to a different IP by a racing DNS
        // response.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .resolve(&host, addr)
            .build()?;

        let response = client.get(current.clone()).send().await?;

        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| anyhow!("redirect response without a usable Location header"))?;
            // Resolve the Location relative to the current URL (handles both
            // absolute and relative redirects).
            current = current.join(location)?;
            continue;
        }

        return Ok(response);
    }

    bail!("too many redirects")
}

/// Resolve the URL's host and return a single validated, publicly-routable
/// socket address to connect to. Errors if the scheme is not http(s), the host
/// does not resolve, or any resolved address is not publicly routable.
async fn validated_addr(url: &Url) -> Result<SocketAddr, Error> {
    match url.scheme() {
        "http" | "https" => {}
        other => bail!("disallowed URL scheme: {other}"),
    }

    let host = url.host_str().ok_or_else(|| anyhow!("URL has no host"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("URL has no port"))?;

    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port)).await?.collect();
    if addrs.is_empty() {
        bail!("host did not resolve to any address");
    }

    // Reject if ANY resolved address is disallowed, so a host that returns a mix
    // of public and internal records cannot be used to slip past the check.
    for addr in &addrs {
        if is_disallowed_ip(addr.ip()) {
            bail!("host resolves to a non-public address: {}", addr.ip());
        }
    }

    Ok(addrs[0])
}

/// Whether an IP address is one we must never let a user-supplied URL reach.
fn is_disallowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_documentation()
                || v4.is_multicast()
                // 100.64.0.0/10 carrier-grade NAT / shared address space
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
                // 192.0.0.0/24 IETF protocol assignments
                || (v4.octets()[0] == 192 && v4.octets()[1] == 0 && v4.octets()[2] == 0)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fc00::/7 unique local addresses
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // fe80::/10 link-local
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // IPv4-mapped addresses (::ffff:0:0/96) — check the embedded v4
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| is_disallowed_ip(IpAddr::V4(v4)))
        }
    }
}
