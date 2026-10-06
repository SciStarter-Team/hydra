// The unconfirmed-email banner (templates/site/verify_banner.stpl.html) opens
// its details panel with a <details>, which the browser toggles unaided. This
// closes the panel on Escape or a click anywhere outside it, the way a pop-up
// is expected to behave.
(function () {
  function openPanels() {
    return document.querySelectorAll("details.verify-banner-details[open]");
  }

  document.addEventListener("click", function (e) {
    openPanels().forEach(function (details) {
      if (!details.contains(e.target)) details.open = false;
    });
  });

  document.addEventListener("keydown", function (e) {
    if (e.key !== "Escape") return;
    openPanels().forEach(function (details) {
      details.open = false;
      details.querySelector("summary").focus();
    });
  });
})();
