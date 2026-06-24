// Outbound external-link click tracking.
//
// Replaces the analytics middleware's server-side href rewriting: instead of
// rewriting every external link to a /-link/<ULID> redirect (which buffered and
// regexed every HTML response and added a blocking NATS call to page render), a
// single delegated click listener reports outbound clicks via sendBeacon and
// lets the link navigate directly.
//
// sendBeacon is used specifically because it survives the page unload that a
// same-tab navigation triggers. Links opt out with `data-no-track` (e.g. social
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
