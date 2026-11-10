// Inside an element with `data-pick-limit="N"`, once N checkboxes are ticked
// the unticked ones are disabled, and untick one and they are enabled again.
// The server renders the boxes disabled to begin with when N are already
// ticked; this keeps them so as boxes are ticked. The listener is on the
// document so that boxes htmx swaps in later work too.
(function () {
  document.addEventListener("change", function (e) {
    if (!(e.target instanceof HTMLInputElement) || e.target.type !== "checkbox") return;

    var group = e.target.closest("[data-pick-limit]");
    if (!group) return;

    var limit = Number(group.dataset.pickLimit);
    var boxes = group.querySelectorAll("input[type=checkbox]");
    var ticked = group.querySelectorAll("input[type=checkbox]:checked").length;
    boxes.forEach(function (box) {
      box.disabled = ticked >= limit && !box.checked;
    });
  });
})();
