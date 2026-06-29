// Shared location-mode toggle for the add forms. Shows only the fields for the
// selected mode; disabled fieldsets keep their controls out of the submission
// (avoids duplicate city/state). Auto-inits every [data-location-picker].
(function () {
  function init(root) {
    if (root.__locationPickerInit) return;
    root.__locationPickerInit = true;

    function apply() {
      var checked = root.querySelector('input[name="location_mode"]:checked');
      var mode = checked ? checked.value : "in_person";
      root.querySelectorAll(".location-fields").forEach(function (fs) {
        var active = fs.getAttribute("data-mode") === mode;
        fs.hidden = !active;
        fs.disabled = !active;
      });
      // The region map is initialized while hidden, so it renders at zero size
      // until its pane is shown — fix it up once the region pane is visible.
      if (mode === "region" && window.RegionEditor) window.RegionEditor.resizeAll();
    }

    root.querySelectorAll('input[name="location_mode"]').forEach(function (r) {
      r.addEventListener("change", apply);
    });
    apply();
  }

  function initAll() {
    document.querySelectorAll("[data-location-picker]").forEach(init);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", initAll);
  } else {
    initAll();
  }
})();
