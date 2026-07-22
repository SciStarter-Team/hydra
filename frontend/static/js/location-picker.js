// Shared location section for the add forms: an "in person or virtual?" mode
// select, a Photon-backed place type-ahead (via /add/place-search), and the
// conditionally revealed region editor. Shows only the fields for the selected
// mode; disabled fieldsets keep their controls out of the submission. The
// "region" mode isn't offered in the dropdown — a link inside the in-person
// pane switches to it (and back). Auto-inits every [data-location-picker].
(function () {
  function init(root) {
    if (root.__locationPickerInit) return;
    root.__locationPickerInit = true;

    var select = root.querySelector('select[name="location_mode"]');

    function apply() {
      var mode = select ? select.value : "in_person";
      root.querySelectorAll(".location-fields").forEach(function (fs) {
        var active = fs.getAttribute("data-mode") === mode;
        fs.hidden = !active;
        fs.disabled = !active;
      });
      // The region map is initialized while hidden, so it renders at zero size
      // until its pane is shown — fix it up once the region pane is visible.
      if (mode === "region" && window.RegionEditor) window.RegionEditor.resizeAll();
      // The URL and location-note fields apply to every mode, but stay out of
      // the way until the in-person-or-virtual question is answered. The note
      // is required for a region, so drawing one opens it.
      var common = root.querySelector("[data-location-common]");
      if (common) common.hidden = mode === "";
      var note = root.querySelector(".location-note");
      if (note && mode === "region") note.open = true;
      applyUrlText(mode);
    }

    // The link/URL field is present in every mode; only its labeling shifts
    // to "join link" phrasing for virtual.
    var urlField = root.querySelector("[data-url-field]");
    function applyUrlText(mode) {
      if (!urlField) return;
      var online = mode === "online";
      var label = urlField.querySelector(".url-label");
      var help = urlField.querySelector(".url-help");
      if (label) label.textContent = urlField.getAttribute(online ? "data-online-label" : "data-default-label");
      if (help) help.textContent = urlField.getAttribute(online ? "data-online-help" : "data-default-help");
    }

    if (select) select.addEventListener("change", apply);

    // "Happens across a wider area?" / "Back to a single place" links switch
    // between the in-person pane and the region editor.
    root.querySelectorAll("[data-mode-link]").forEach(function (btn) {
      btn.addEventListener("click", function () {
        if (!select) return;
        select.value = btn.getAttribute("data-mode-link");
        apply();
      });
    });

    initPlaceSearch(root);
    apply();
  }

  // --- Place type-ahead -----------------------------------------------------
  // Debounced queries to /add/place-search; picking a suggestion copies its
  // structured components into the address fields (kept behind the details
  // disclosure for corrections) and pins the coordinates in the hidden
  // latitude/longitude inputs.
  function initPlaceSearch(root) {
    var wrap = root.querySelector("[data-place-search]");
    if (!wrap) return;
    var input = wrap.querySelector('input[type="search"]');
    var list = wrap.querySelector(".place-search-results");
    if (!input || !list) return;

    var fields = {
      location_name: root.querySelector('input[name="location_name"]'),
      address: root.querySelector('input[name="address"]'),
      city: root.querySelector('input[name="city"]'),
      state: root.querySelector('input[name="state"]'),
      zip: root.querySelector('input[name="zip"]'),
      country: root.querySelector('input[name="country"]'),
      latitude: root.querySelector('input[name="latitude"]'),
      longitude: root.querySelector('input[name="longitude"]'),
    };
    var timer = null;
    var seq = 0;

    function close() {
      list.hidden = true;
      list.innerHTML = "";
      input.setAttribute("aria-expanded", "false");
    }

    function pick(s) {
      if (fields.location_name) fields.location_name.value = s.location_name;
      if (fields.address) fields.address.value = s.address;
      if (fields.city) fields.city.value = s.city;
      if (fields.state) fields.state.value = s.state;
      if (fields.zip) fields.zip.value = s.zip;
      if (fields.country) fields.country.value = s.country;
      if (fields.latitude) fields.latitude.value = s.latitude;
      if (fields.longitude) fields.longitude.value = s.longitude;
      input.value = s.context ? s.name + ", " + s.context : s.name;
      close();
      // Photon's OSM feature often lacks a postcode; backfill it with one
      // reverse lookup, unless the visitor has typed one in the meantime.
      if (!s.zip && fields.zip) {
        fetch("/add/zip-lookup?lat=" + s.latitude + "&lon=" + s.longitude, { headers: { Accept: "application/json" } })
          .then(function (r) { return r.ok ? r.json() : null; })
          .then(function (data) {
            if (data && data.zip && !fields.zip.value) fields.zip.value = data.zip;
          })
          .catch(function () {});
      }
    }

    function render(suggestions) {
      list.innerHTML = "";
      if (!suggestions.length) {
        close();
        return;
      }
      suggestions.forEach(function (s) {
        var li = document.createElement("li");
        li.setAttribute("role", "option");
        var btn = document.createElement("button");
        btn.type = "button";
        var name = document.createElement("span");
        name.className = "place-name";
        name.textContent = s.name;
        btn.appendChild(name);
        // The kind ("City", "County") is often the only difference between
        // same-named places, so it leads the context line.
        var contextText = [s.kind, s.context].filter(Boolean).join(" · ");
        if (contextText) {
          var context = document.createElement("span");
          context.className = "place-context";
          context.textContent = contextText;
          btn.appendChild(context);
        }
        btn.addEventListener("click", function () { pick(s); });
        li.appendChild(btn);
        list.appendChild(li);
      });
      list.hidden = false;
      input.setAttribute("aria-expanded", "true");
    }

    function search() {
      var q = input.value.trim();
      if (q.length < 3) {
        close();
        return;
      }
      var current = ++seq;
      fetch("/add/place-search?q=" + encodeURIComponent(q), { headers: { Accept: "application/json" } })
        .then(function (r) { return r.ok ? r.json() : []; })
        .then(function (suggestions) {
          if (current === seq) render(suggestions || []);
        })
        .catch(function () {});
    }

    input.addEventListener("input", function () {
      clearTimeout(timer);
      timer = setTimeout(search, 300);
    });
    input.addEventListener("keydown", function (e) {
      if (e.key === "Enter") {
        e.preventDefault();
        var first = list.querySelector("button");
        if (first && !list.hidden) first.click();
      } else if (e.key === "Escape") {
        close();
      }
    });
    document.addEventListener("click", function (e) {
      if (!wrap.contains(e.target)) close();
    });

    // A hand-corrected address may no longer match the picked coordinates;
    // clear them so the server geocodes the corrected address instead.
    ["address", "city", "state", "zip", "country"].forEach(function (key) {
      if (!fields[key]) return;
      fields[key].addEventListener("input", function () {
        if (fields.latitude) fields.latitude.value = "";
        if (fields.longitude) fields.longitude.value = "";
      });
    });
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
