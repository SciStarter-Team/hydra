// A `details.quiet-disclosure` is a "+ Add something" link that reveals its
// fields in place of itself: once open, its summary is hidden. When the
// visitor opened it from the summary, focus would be left on a hidden element,
// so it moves on to the first field revealed.
(function () {
  document.addEventListener("toggle", function (e) {
    var details = e.target;
    if (!(details instanceof HTMLDetailsElement)) return;
    if (!details.classList.contains("quiet-disclosure") || !details.open) return;
    var summary = details.querySelector(":scope > summary");
    if (document.activeElement !== summary) return;
    var first = details.querySelector("input:not([type=hidden]), select, textarea");
    if (first) first.focus();
  }, true);
})();
