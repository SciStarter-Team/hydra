// A button with `data-copies="ID"` copies the text of the element with that id.
// Its `[aria-live]` child shows `data-copied` for a moment afterward. Where the
// clipboard is unavailable or refused, the text is selected instead, so the
// reader can copy it themselves. The listener is on the document so that buttons
// htmx swaps in later work too.
(function () {
  var resting = new WeakMap();

  function select_text(source) {
    var range = document.createRange();
    range.selectNodeContents(source);
    var selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
  }

  document.addEventListener("click", async function (e) {
    var button = e.target.closest("[data-copies]");
    if (!button) return;

    var source = document.getElementById(button.dataset.copies);
    if (!source) return;

    try {
      await navigator.clipboard.writeText(source.textContent);
    } catch (err) {
      select_text(source);
      return;
    }

    var status = button.querySelector("[aria-live]");
    var state = resting.get(button);
    if (state) {
      clearTimeout(state.timer);
    } else {
      state = { text: status.textContent };
      resting.set(button, state);
    }

    status.textContent = button.dataset.copied;
    button.classList.add("copied");
    state.timer = setTimeout(function () {
      status.textContent = state.text;
      button.classList.remove("copied");
      resting.delete(button);
    }, 2000);
  });
})();
