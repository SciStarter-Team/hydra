// Date/time rows (site/date_row.stpl.html), in the add event form and the
// event page's date editor: the time zone, named as it is on the date it
// goes with, the end-time chip, the add form's repetition of each date, and
// a saved date shown on the clocks of whichever zone is chosen. A zone
// outside a row (when registration opens) is named as of the date and time
// in its own `[data-zone-scope]`.
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

  // The zone's short name ("PDT", "GMT+3") at `at`, or now, as the event
  // page names it (js/local_time.js): New York is EDT in July and EST in
  // January.
  var shortName = window.LocalTime.zoneName;

  function control(row, part) {
    return row.querySelector('[name="' + part + '"], [name$="[' + part + ']"]');
  }

  // The numeric parts of instant `t` (milliseconds) on `zone`'s clocks.
  function zoneParts(zone, t) {
    var p = {};
    new Intl.DateTimeFormat("en-US", {
      timeZone: zone, hourCycle: "h23",
      year: "numeric", month: "2-digit", day: "2-digit",
      hour: "2-digit", minute: "2-digit", second: "2-digit"
    }).formatToParts(new Date(t)).forEach(function (part) {
      if (part.type !== "literal") p[part.type] = parseInt(part.value, 10);
    });
    // Some engines spell midnight "24" even under h23.
    if (p.hour === 24) p.hour = 0;
    return p;
  }

  // Milliseconds to add to UTC to get `zone`'s wall-clock time at `t`.
  function zoneOffset(zone, t) {
    var p = zoneParts(zone, t);
    var wholeSeconds = t - (((t % 1000) + 1000) % 1000);
    return Date.UTC(p.year, p.month - 1, p.day, p.hour, p.minute, p.second) - wholeSeconds;
  }

  // The instant `date` ("YYYY-MM-DD") and `time` ("HH:MM") name on `zone`'s
  // clocks, or null when they name none: the earlier, where a clock change
  // repeats the time, and the time after the gap, where it skips it. Each
  // offset in force a day either side proposes an instant; a proposal is
  // real when the zone uses that offset at it.
  function zonedInstant(date, time, zone) {
    var d = date.split("-").map(Number);
    var h = time.split(":").map(Number);
    if (d.length !== 3 || h.length < 2 || d.concat(h.slice(0, 2)).some(isNaN)) return null;
    var guess = Date.UTC(d[0], d[1] - 1, d[2], h[0], h[1]);
    var day = 24 * 60 * 60 * 1000;
    var proposals = [guess - zoneOffset(zone, guess - day), guess - zoneOffset(zone, guess + day)];
    var real = proposals.filter(function (t) { return zoneOffset(zone, t) === guess - t; });
    return real.length > 0 ? Math.min.apply(null, real) : Math.max.apply(null, proposals);
  }

  // The moment the date and time beside a zone name on its clocks (noon,
  // before a time is given), for the zone's name as of then; null, for now,
  // before a date is. They are the first date and time in the zone's scope:
  // a row's start, or when registration opens.
  function zoneMoment(select) {
    var scope = select.closest("[data-zone-scope]");
    var date = scope && scope.querySelector('input[type="date"]');
    if (!date || !date.value || !select.value) return null;
    var time = scope.querySelector('input[type="time"]');
    try {
      return zonedInstant(date.value, time && time.value ? time.value : "12:00", select.value);
    } catch (e) {
      return null;
    }
  }

  // The field shows the zone's short name over the full list, which lies
  // invisibly across the field and opens on click.
  function syncTz(select) {
    if (!select) return;
    var field = select.closest("[data-tz-field]");
    if (!field) return;
    field.classList.add("tz-enhanced");
    var short = field.querySelector(".tz-short");
    if (short) short.textContent = select.value ? shortName(select.value, zoneMoment(select)) : "";
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
  // takes its place. Most events end the day they start, so, as Facebook
  // does, the end date starts out as the start date and the end time is
  // what's asked for.
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
      var endDate = control(row, "end_date");
      if (endDate && !endDate.value) endDate.value = control(row, "begin_date").value;
      var endTime = control(row, "end_time");
      if (endTime) endTime.focus();
    } else if (!on && link) {
      link.focus();
    }
  }

  // "Repeat until" and its help show only while the row repeats; the server
  // ignores the date while it doesn't.
  function repeatUntilShown(row) {
    var block = row.querySelector("[data-repeat]");
    if (!block) return;
    var frequency = block.querySelector("[data-repeat-frequency]");
    var repeating = frequency.value !== "once";
    block.querySelectorAll("[data-repeat-only]").forEach(function (el) { el.hidden = !repeating; });
    var help = block.querySelector("p[data-repeat-only]");
    if (repeating && help) {
      frequency.setAttribute("aria-describedby", help.id);
    } else {
      frequency.removeAttribute("aria-describedby");
    }
  }

  // "Make this a recurring event" shows a row's repetition, which stays;
  // "Never" among the frequencies stops it repeating. The link is gone once
  // used, so focus moves to the frequency, unless `quietly`. Put away
  // (`on` false), the row doesn't repeat: a blank row's.
  function setRepeat(row, on, quietly) {
    var block = row.querySelector("[data-repeat]");
    var chip = row.querySelector("[data-repeat-toggle]");
    if (!block || !chip) return;
    if (!on) {
      block.querySelector("[data-repeat-frequency]").value = "once";
      control(row, "repeat_until").value = "";
      repeatUntilShown(row);
    }
    block.hidden = !on;
    chip.hidden = on;
    if (!quietly) block.querySelector("[data-repeat-frequency]").focus();
  }

  document.addEventListener("click", function (e) {
    var row = e.target.closest && e.target.closest("[data-date-row]");
    if (!row) return;
    // The chips hold icons, which a click can land on.
    // Either changes the saved date, as typing over it does, and the end
    // date "Add end time" fills in is the start's on the clocks shown now.
    if (e.target.closest(".end-toggle")) {
      row.dataset.touched = "1";
      setEnd(row, true);
    } else if (e.target.closest(".end-remove")) {
      row.dataset.touched = "1";
      setEnd(row, false);
    } else if (e.target.closest("[data-repeat-toggle]")) {
      setRepeat(row, true);
    }
  });

  // A copy of a row, as a new date/time: nothing entered, no end, not
  // repeating, its zone not chosen by hand, and no mark of a refusal that
  // was about the row it was copied from.
  function blank(row) {
    row.querySelectorAll("input").forEach(function (el) {
      if (el.type === "checkbox") { el.checked = false; } else { el.value = ""; }
    });
    // The refusal's messages (`FormError::id`, which ends "-problem") no
    // longer describe it.
    row.querySelectorAll("[aria-invalid]").forEach(function (el) {
      el.removeAttribute("aria-invalid");
      var described = (el.getAttribute("aria-describedby") || "").split(/\s+/).filter(function (id) {
        return id && !/-problem$/.test(id);
      });
      if (described.length) {
        el.setAttribute("aria-describedby", described.join(" "));
      } else {
        el.removeAttribute("aria-describedby");
      }
    });
    row.querySelectorAll("select.tz-select").forEach(function (select) {
      select.selectedIndex = 0;
      delete select.dataset.userSet;
    });
    row.querySelectorAll(".end-block").forEach(function (end) { end.hidden = true; });
    row.querySelectorAll(".end-toggle").forEach(function (link) { link.hidden = false; });
    row.querySelectorAll("[data-date-row]").forEach(function (dateRow) {
      delete dateRow.dataset.touched;
      setRepeat(dateRow, false, true);
    });
  }

  // Typing over a saved date's date or time changes it, and the zone is
  // named afresh for the date and time it goes with.
  document.addEventListener("input", function (e) {
    if (!e.target.matches || !e.target.matches("[data-zone-scope] input")) return;
    var row = e.target.closest("[data-date-row]");
    if (row && e.isTrusted) row.dataset.touched = "1";
    var select = e.target.closest("[data-zone-scope]").querySelector("select.tz-select");
    if (select) syncTz(select);
  });

  // A hand-picked zone is an explicit choice; remember not to overwrite it.
  document.addEventListener("change", function (e) {
    if (e.target.matches && e.target.matches("[data-repeat-frequency]")) {
      repeatUntilShown(e.target.closest("[data-date-row]"));
    }
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

  window.DateRows = { ensureTz: ensureTz, blank: blank };

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
