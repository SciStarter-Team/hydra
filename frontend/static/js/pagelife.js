// Analytics that only the browser can observe. Both halves report through
// sendBeacon, which survives the page unload that a same-tab navigation
// triggers and never holds up the page.

// Outbound external-link clicks.
//
// A single delegated click listener reports outbound clicks to /-clickthrough
// and lets the link navigate directly, so links stay plain hrefs and the server
// never has to rewrite them. Links opt out with `data-no-track` (e.g. social
// share-intent links, which are recorded as shares, not clickthroughs).
(function () {
  document.addEventListener(
    "click",
    function (event) {
      var anchor = event.target && event.target.closest
        ? event.target.closest("a[href]")
        : null;
      if (!anchor || anchor.hasAttribute("data-no-track")) return;

      // anchor.href is the browser-resolved absolute URL.
      var url = anchor.href;
      if (!/^https?:\/\//i.test(url)) return;

      // Same-origin links are navigations, not outbound clickthroughs.
      if (url === location.origin || url.indexOf(location.origin + "/") === 0) return;

      if (!navigator.sendBeacon) return;
      try {
        navigator.sendBeacon(
          "/-clickthrough",
          new Blob(
            [JSON.stringify({ to_url: url, from_path: location.pathname })],
            { type: "application/json" }
          )
        );
      } catch (e) {}
    },
    true // capture phase, so we still see clicks if a handler stops propagation
  );
})();

// Views of pages the browser fetched speculatively.
//
// A prefetched or prerendered page is requested before anyone decides to look
// at it, so the server does not count that request (is_page_request in
// src/middleware/analytics.rs). When the page is then shown, the browser uses
// the response it already has and asks the server for nothing, so the view is
// reported to /-pageview from here instead.
(function () {
  if (!navigator.sendBeacon) return;

  function report() {
    try {
      navigator.sendBeacon(
        "/-pageview",
        new Blob(
          [
            JSON.stringify({
              path: location.pathname,
              query: location.search.slice(1),
              referrer: document.referrer,
            }),
          ],
          { type: "application/json" }
        )
      );
    } catch (e) {}
  }

  // Still hidden in a prerender: report when, and only if, it is shown.
  if (document.prerendering) {
    document.addEventListener("prerenderingchange", report, { once: true });
    return;
  }

  var nav =
    performance.getEntriesByType && performance.getEntriesByType("navigation")[0];
  if (!nav) return;

  // A prerendered page this script only ran in after it was shown.
  var prerendered = nav.activationStart > 0;

  // Prefetched by speculation rules or a prefetch proxy.
  var prefetched = nav.deliveryType === "navigational-prefetch";

  // Taken from the HTTP cache without asking the server. Pages are sent with
  // no freshness lifetime and no validators, so for a fresh navigation this
  // happens only when Chrome reuses a <link rel=prefetch> response, which it
  // does for a few minutes regardless of cacheability. Other browsers keep
  // ordinary cache semantics and fetch the page again, which the server
  // counts. Giving pages a freshness lifetime would break that: a browser
  // without `deliveryType` could then show a prefetched page that neither
  // side counts. Back/forward loads are excluded: those are revisits, which
  // the server has never counted.
  var cached = nav.deliveryType === "cache" && nav.type === "navigate";

  if (prerendered || prefetched || cached) report();
})();
