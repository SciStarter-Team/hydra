// The answers about signing up for an event, what attending costs and who
// can participate (site/registration_fields.stpl.html,
// site/admission_fields.stpl.html, site/attendance_fields.stpl.html), in the
// add event form and the event page's editors: each answer shows the fields
// it asks for under it. Listens on the document, so fields swapped in by
// HTMX need nothing of their own.
(function () {
  document.addEventListener("change", function (e) {
    var target = e.target;
    if (!target.matches) return;

    // Who can participate: an event for a specific audience is asked which,
    // and what ages it suits, and moves there.
    if (target.matches('[data-attendance] input[name="attendance"]')) {
      var attendance = target.closest("[data-attendance]");
      var limited = target.value === "limited";
      attendance.querySelectorAll("[data-limited-only]").forEach(function (part) {
        part.hidden = !limited;
      });
      if (limited) attendance.querySelector('input[name="limited_to"]').focus();
      return;
    }

    // Registration: the link is required while registration is, and when it
    // opens is asked unless nobody registers.
    if (target.matches('[data-registration] input[name="registration"]')) {
      var registration = target.closest("[data-registration]");
      var link = registration.querySelector('input[name="ticket_url"]');
      if (link) link.required = target.value === "required";
      var ticketing = registration.querySelector("[data-ticketing]");
      if (ticketing) {
        var asked = target.value !== "none";
        ticketing.hidden = !asked;
        ticketing.querySelector("fieldset").disabled = !asked;
      }
      return;
    }

    // Admission: the price, or the suggested donation, under the answer
    // that asks it, carrying what was typed from one to the other; the field
    // not asked is disabled, so as not to submit.
    if (target.matches('[data-admission] input[name="admission"]')) {
      var admission = target.closest("[data-admission]");
      var fields = admission.querySelectorAll("[data-cost-for]");
      var typed = "";
      fields.forEach(function (field) {
        if (!field.disabled) typed = field.querySelector("input").value;
      });
      fields.forEach(function (field) {
        var asked = field.getAttribute("data-cost-for") === target.value;
        field.hidden = !asked;
        field.disabled = !asked;
        if (asked) field.querySelector("input").value = typed;
      });
    }
  });
})();
