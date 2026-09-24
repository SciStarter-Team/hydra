// Date and time fields open their browser's own picker when clicked anywhere,
// not only on the small button inside them. (Chrome's button is hidden by the
// stylesheet — it would sit beside the field's own leading icon — which left
// no way to open the picker by pointer; Firefox's button can't be hidden from
// CSS, so there it is a second way in.)
//
// The browser's picker is the only one: a second, hand-built calendar meant
// two calendars at once wherever the native button survives, and it could not
// match the browser's own keyboard and screen-reader handling of the field.
(function () {
  document.addEventListener("click", function (e) {
    var input = e.target.closest && e.target.closest('form input[type="date"], form input[type="time"]');
    if (!input || !input.showPicker) return;
    // showPicker() needs a user gesture (this click is one) and throws if the
    // field is disabled or the picker is already up; neither is worth acting on.
    try {
      input.showPicker();
    } catch (err) {}
  });
})();
