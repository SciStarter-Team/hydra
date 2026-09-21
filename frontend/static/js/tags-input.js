// Shared tag-pill widget for the add forms. The selected set lives in the
// browser (the entity isn't created yet) in the hidden `.tags-value` input;
// autocomplete suggestions come from the server via HTMX. Pills get a stable
// hash-based color. Auto-inits every [data-tag-input].
(function () {
  function hue(s) {
    var h = 0;
    for (var i = 0; i < s.length; i++) { h = (h * 31 + s.charCodeAt(i)) >>> 0; }
    return h % 360;
  }

  function init(root) {
    if (root.__tagInputInit) return;
    root.__tagInputInit = true;

    var hidden = root.querySelector(".tags-value");
    var pillsEl = root.querySelector(".tag-pills");
    var input = root.querySelector(".tag-input");
    var suggestions = root.querySelector(".tag-suggestions");
    var status = root.querySelector("[data-tag-status]");
    if (!hidden || !pillsEl || !input) return;

    var tags = hidden.value.split(",").map(function (t) { return t.trim(); }).filter(Boolean);

    function announce(text) {
      if (status) status.textContent = text;
    }

    function render() {
      pillsEl.innerHTML = "";
      tags.forEach(function (t) {
        var pill = document.createElement("li");
        pill.className = "inline-flex items-center gap-1 px-2 py-1 rounded-full text-sm font-medium text-white";
        // Dark enough that white text reads at 4.5:1 or better whatever the hue
        // (yellows are the lightest).
        pill.style.backgroundColor = "hsl(" + hue(t) + ", 70%, 28%)";
        pill.appendChild(document.createTextNode(t));
        var x = document.createElement("button");
        x.type = "button";
        x.className = "ml-1 leading-none font-bold";
        x.textContent = "×";
        x.setAttribute("aria-label", "Remove tag " + t);
        x.addEventListener("click", function () {
          tags = tags.filter(function (other) { return other !== t; });
          render();
          announce("Removed tag " + t);
          // The button is gone; carry on from the tag input.
          input.focus();
        });
        pill.appendChild(x);
        pillsEl.appendChild(pill);
      });
      hidden.value = tags.join(",");
    }

    function add(tag) {
      tag = (tag || "").trim();
      var added = tag && tags.indexOf(tag) === -1;
      if (added) tags.push(tag);
      input.value = "";
      if (suggestions) suggestions.innerHTML = "";
      render();
      if (added) announce("Added tag " + tag);
      // A picked suggestion button is gone; carry on from the tag input.
      input.focus();
    }

    input.addEventListener("keydown", function (e) {
      if (e.key === "Enter") { e.preventDefault(); add(input.value); }
    });
    if (suggestions) {
      suggestions.addEventListener("click", function (e) {
        var btn = e.target.closest(".tag-suggestion");
        if (btn) add(btn.getAttribute("data-tag"));
      });
    }

    render();
  }

  function initAll() {
    document.querySelectorAll("[data-tag-input]").forEach(init);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", initAll);
  } else {
    initAll();
  }
})();
