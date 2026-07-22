// Inline calendar popover for date fields. The site's CSS suppresses the
// native picker indicator, so this supplies the calendar: focusing (or
// clicking) any `form input[type="date"]` opens a month-grid popover anchored
// below it; picking a day writes the ISO value back to the input. Typing in
// the input still works and the grid follows along. A single shared popover
// element serves every date input via event delegation, so rows cloned after
// load (e.g. "+ Add another date/time") need no re-initialization.
(function () {
  var MONTHS = ["January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December"];
  var WEEKDAYS = ["S", "M", "T", "W", "T", "F", "S"];

  var pop = null;
  var current = null; // the input the popover is serving
  var viewYear = 0;
  var viewMonth = 0; // 0-based

  function ensurePopover() {
    if (pop) return;
    pop = document.createElement("div");
    pop.className = "cal-popover";
    pop.hidden = true;
    // Keep focus in the input while interacting with the popover.
    pop.addEventListener("mousedown", function (e) { e.preventDefault(); });
    pop.addEventListener("click", function (e) {
      var btn = e.target.closest("button");
      if (!btn || !current) return;
      if (btn.classList.contains("cal-prev") || btn.classList.contains("cal-next")) {
        viewMonth += btn.classList.contains("cal-next") ? 1 : -1;
        if (viewMonth < 0) { viewMonth = 11; viewYear--; }
        if (viewMonth > 11) { viewMonth = 0; viewYear++; }
        render();
      } else if (btn.classList.contains("cal-day")) {
        current.value = btn.getAttribute("data-date");
        current.dispatchEvent(new Event("input", { bubbles: true }));
        current.dispatchEvent(new Event("change", { bubbles: true }));
        close();
      }
    });
    document.body.appendChild(pop);
  }

  function parseISO(value) {
    var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value || "");
    if (!m) return null;
    return { y: +m[1], mo: +m[2] - 1, d: +m[3] };
  }

  function pad(n) { return (n < 10 ? "0" : "") + n; }

  function render() {
    var selected = parseISO(current.value);
    var now = new Date();

    var html = '<div class="cal-header">' +
      '<button type="button" class="cal-prev" aria-label="Previous month">&#8249;</button>' +
      '<span class="cal-title">' + MONTHS[viewMonth] + " " + viewYear + "</span>" +
      '<button type="button" class="cal-next" aria-label="Next month">&#8250;</button>' +
      "</div>" +
      '<div class="cal-grid">';
    WEEKDAYS.forEach(function (d) { html += '<span class="cal-dow">' + d + "</span>"; });

    var firstDay = new Date(viewYear, viewMonth, 1).getDay();
    var daysInMonth = new Date(viewYear, viewMonth + 1, 0).getDate();
    for (var i = 0; i < firstDay; i++) html += "<span></span>";
    for (var day = 1; day <= daysInMonth; day++) {
      var iso = viewYear + "-" + pad(viewMonth + 1) + "-" + pad(day);
      var classes = "cal-day";
      if (selected && selected.y === viewYear && selected.mo === viewMonth && selected.d === day) {
        classes += " is-selected";
      }
      if (now.getFullYear() === viewYear && now.getMonth() === viewMonth && now.getDate() === day) {
        classes += " is-today";
      }
      html += '<button type="button" class="' + classes + '" data-date="' + iso + '">' + day + "</button>";
    }
    html += "</div>";
    pop.innerHTML = html;
  }

  function position(input) {
    var rect = input.getBoundingClientRect();
    pop.style.left = rect.left + window.scrollX + "px";
    pop.style.top = rect.bottom + window.scrollY + 4 + "px";
  }

  function open(input) {
    ensurePopover();
    current = input;
    var selected = parseISO(input.value);
    var base = selected ? new Date(selected.y, selected.mo, 1) : new Date();
    viewYear = base.getFullYear();
    viewMonth = base.getMonth();
    render();
    position(input);
    pop.hidden = false;
  }

  function close() {
    if (pop) pop.hidden = true;
    current = null;
  }

  function isDateInput(el) {
    return el instanceof HTMLInputElement && el.type === "date" && el.form;
  }

  document.addEventListener("focusin", function (e) {
    if (isDateInput(e.target)) {
      open(e.target);
    } else if (pop && !pop.hidden && !pop.contains(e.target)) {
      close();
    }
  });
  // Re-open on click when the input is already focused (e.g. closed via Escape).
  document.addEventListener("click", function (e) {
    if (isDateInput(e.target) && (!pop || pop.hidden)) open(e.target);
  });
  document.addEventListener("mousedown", function (e) {
    if (!pop || pop.hidden) return;
    if (pop.contains(e.target) || e.target === current) return;
    close();
  });
  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape") close();
  });
  // Keep the grid in step with hand-typed values.
  document.addEventListener("input", function (e) {
    if (!pop || pop.hidden || e.target !== current) return;
    var typed = parseISO(current.value);
    if (typed) {
      viewYear = typed.y;
      viewMonth = typed.mo;
    }
    render();
  });
  window.addEventListener("resize", close);
})();
