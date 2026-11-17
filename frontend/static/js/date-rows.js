// Date/time rows (site/date_row.stpl.html), in the add event form and the
// event page's date editor: the time-zone chip, the end-time chip, and a
// saved date shown on the clocks of whichever zone is chosen.
(function () {
  // Preselect a sensible time zone: the zone of a venue picked in the
  // location search when there is one, else the visitor's browser zone.
  // Never overrides a zone the visitor chose themselves.
  var browserTz = "";
  try { browserTz = Intl.DateTimeFormat().resolvedOptions().timeZone || ""; } catch (e) {}
  var placeTz = "";

  function hasOption(select, tz) {
    return Array.prototype.some.call(select.options, function (o) { return o.value === tz; });
  }

  // The zone's short name ("PDT", "GMT+3") at `at`, or now: New York is EDT
  // in July and EST in January.
  function shortName(tz, at) {
    try {
      var parts = new Intl.DateTimeFormat("en-US", { timeZone: tz, timeZoneName: "short" })
        .formatToParts(at ? new Date(at) : new Date());
      var name = parts.find(function (p) { return p.type === "timeZoneName"; });
      return name ? name.value : tz;
    } catch (e) {
      return tz;
    }
  }

  function control(row, part) {
    return row.querySelector('[name="' + part + '"], [name$="[' + part + ']"]');
  }

  // The moment a row's start names, roughly (for the zone's name as of that
  // date); null when it names none yet.
  function rowMoment(row) {
    var date = control(row, "begin_date");
    if (!date || !date.value) return null;
    var time = control(row, "begin_time");
    return date.value + "T" + (time && time.value ? time.value : "12:00") + ":00Z";
  }

  // The field shows the zone's short name over the full list, which lies
  // invisibly across the field and opens on click.
  function syncTz(select) {
    if (!select) return;
    var field = select.closest("[data-tz-field]");
    if (!field) return;
    field.classList.add("tz-enhanced");
    var short = field.querySelector(".tz-short");
    var row = select.closest("[data-date-row]");
    // A chip names the field until a zone is chosen.
    if (short) {
      short.textContent = select.value
        ? shortName(select.value, row && rowMoment(row))
        : short.getAttribute("data-placeholder") || "";
    }
  }

  function addOption(select, tz) {
    if (tz && !hasOption(select, tz)) {
      var opt = document.createElement("option");
      opt.value = tz;
      opt.textContent = tz;
      select.appendChild(opt);
    }
  }

  function ensureTz(select) {
    if (!select) return;
    addOption(select, browserTz);
    if (!select.value) {
      if (placeTz && hasOption(select, placeTz)) {
        select.value = placeTz;
      } else if (browserTz) {
        select.value = browserTz;
      }
    }
    syncTz(select);
  }

  // `iso` on `tz`'s clocks, as a date input and a time input write it.
  function wall(iso, tz) {
    var parts = {};
    new Intl.DateTimeFormat("en-CA", {
      timeZone: tz, year: "numeric", month: "2-digit", day: "2-digit",
      hour: "2-digit", minute: "2-digit", hourCycle: "h23"
    }).formatToParts(new Date(iso)).forEach(function (p) { parts[p.type] = p.value; });
    return { date: parts.year + "-" + parts.month + "-" + parts.day, time: parts.hour + ":" + parts.minute };
  }

  // A saved date left as it was is still the moment it was saved as, shown
  // on `tz`'s clocks; once its date or time is typed over, they mean the
  // time on the chosen zone's clocks instead.
  function showStored(row, tz) {
    var begin = row.getAttribute("data-stored-begin");
    if (!begin || row.dataset.touched || !tz) return;
    var end = row.getAttribute("data-stored-end");
    try {
      var b = wall(begin, tz);
      control(row, "begin_date").value = b.date;
      control(row, "begin_time").value = b.time;
      if (end && end !== begin) {
        var e = wall(end, tz);
        control(row, "end_date").value = e.date;
        control(row, "end_time").value = e.time;
      }
    } catch (err) {}
  }

  // Move `row` onto `tz`'s clocks.
  function follow(row, tz) {
    var select = row.querySelector("select.tz-select");
    if (!select || !tz) return;
    addOption(select, tz);
    showStored(row, tz);
    select.value = tz;
    syncTz(select);
  }

  function init(scope) {
    scope.querySelectorAll("select.tz-select").forEach(function (select) {
      if (select.dataset.dateRowsReady) return;
      select.dataset.dateRowsReady = "1";
      ensureTz(select);
    });
    // A saved date the server could only show on UTC's clocks goes onto the
    // viewer's own.
    scope.querySelectorAll("[data-date-row][data-follow-viewer]").forEach(function (row) {
      row.removeAttribute("data-follow-viewer");
      follow(row, browserTz);
    });
  }

  // "Add end time" reveals a row's end fields (and checks the hidden has_end
  // box that carries the state to the server); "Remove end time" undoes it.
  // Each hides the control that was used, so focus moves to the one that
  // takes its place.
  function setEnd(row, on) {
    var toggle = row.querySelector(".has-end-toggle");
    var block = row.querySelector(".end-block");
    var link = row.querySelector(".end-toggle");
    if (toggle) toggle.checked = on;
    if (block) block.hidden = !on;
    if (link) link.hidden = on;
    if (!on && block) {
      block.querySelectorAll("input").forEach(function (el) { el.value = ""; });
    }
    if (on && block) {
      var endDate = block.querySelector("input");
      if (endDate) endDate.focus();
    } else if (!on && link) {
      link.focus();
    }
  }
  document.addEventListener("click", function (e) {
    var row = e.target.closest && e.target.closest("[data-date-row]");
    if (!row) return;
    // The chips hold icons, which a click can land on.
    if (e.target.closest(".end-toggle")) {
      setEnd(row, true);
    } else if (e.target.closest(".end-remove")) {
      row.dataset.touched = "1";
      setEnd(row, false);
    }
  });

  // Typing over a saved date's date or time changes it.
  document.addEventListener("input", function (e) {
    if (!e.isTrusted || !e.target.matches || !e.target.matches("[data-date-row] input")) return;
    e.target.closest("[data-date-row]").dataset.touched = "1";
    var select = e.target.closest("[data-date-row]").querySelector("select.tz-select");
    if (select) syncTz(select);
  });

  // A hand-picked zone is an explicit choice; remember not to overwrite it.
  document.addEventListener("change", function (e) {
    if (e.target.matches && e.target.matches("select.tz-select")) {
      if (e.isTrusted) e.target.dataset.userSet = "1";
      var row = e.target.closest("[data-date-row]");
      if (row) showStored(row, e.target.value);
      syncTz(e.target);
    }
  });

  // The location picker announces the zone of a picked venue; move every
  // row whose zone wasn't chosen by hand onto it (regions and virtual events
  // announce nothing).
  document.addEventListener("ss:place-timezone", function (e) {
    placeTz = e.detail.timezone;
    document.querySelectorAll("select.tz-select").forEach(function (select) {
      if (select.dataset.userSet || !hasOption(select, placeTz)) return;
      var row = select.closest("[data-date-row]");
      if (row) {
        follow(row, placeTz);
      } else {
        select.value = placeTz;
        syncTz(select);
      }
    });
  });

  window.DateRows = { ensureTz: ensureTz };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () { init(document); });
  } else {
    init(document);
  }
  // Fired on each root of swapped-in content once it's in the page (the
  // settle event goes to the swap's target, which an outerHTML swap has
  // already taken out of it).
  document.addEventListener("htmx:after:process", function (e) {
    if (e.target && e.target.querySelectorAll) init(e.target);
  });
})();
