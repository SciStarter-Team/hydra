// A `<turnstile-widget sitekey=…>` runs Cloudflare's Turnstile check inside
// the form it sits in, which then submits the check's token as
// `cf-turnstile-response`. Optional attributes `appearance` and `language`
// pass through to Turnstile.
//
// When Turnstile can't run (a blocked script or a failed challenge) the form
// still works: the server defers the account behind an emailed completion
// link instead. The widget then reveals its `[data-turnstile-fallback]`
// child, which says so.
//
// Rendering is explicit, so a widget swapped in by htmx gets its own check,
// and one swapped out is removed from Turnstile.
(function () {
  if (customElements.get("turnstile-widget")) return;

  var API = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
  var loading = null;

  function turnstile() {
    if (window.turnstile) return Promise.resolve(window.turnstile);
    if (!loading) {
      loading = new Promise(function (resolve, reject) {
        var script = document.createElement("script");
        script.src = API;
        script.async = true;
        script.onload = function () { resolve(window.turnstile); };
        script.onerror = function () {
          loading = null;
          script.remove();
          reject(new Error("Turnstile could not be loaded"));
        };
        document.head.appendChild(script);
      });
    }
    return loading;
  }

  class TurnstileWidget extends HTMLElement {
    connectedCallback() {
      if (this.challenge) return;
      this.challenge = document.createElement("div");
      this.prepend(this.challenge);

      var self = this;
      turnstile().then(function (api) {
        if (!self.isConnected) return;
        var options = {
          sitekey: self.getAttribute("sitekey"),
          callback: function () { self.passed(); },
          "expired-callback": function () { self.reset(); },
          "error-callback": function () { self.failed(); },
        };
        if (self.hasAttribute("appearance")) options.appearance = self.getAttribute("appearance");
        if (self.hasAttribute("language")) options.language = self.getAttribute("language");
        self.widgetId = api.render(self.challenge, options);
      }, function () { self.failed(); });
    }

    disconnectedCallback() {
      if (this.widgetId !== undefined && window.turnstile) window.turnstile.remove(this.widgetId);
      this.widgetId = undefined;
      if (this.challenge) this.challenge.remove();
      this.challenge = null;
    }

    fallback(shown) {
      var note = this.querySelector("[data-turnstile-fallback]");
      if (note) note.hidden = !shown;
    }

    // Once passed there is nothing to show, so the widget and its margins
    // step out of the form's layout.
    passed() {
      this.style.display = "none";
      this.fallback(false);
    }

    reset() {
      this.style.display = "";
    }

    failed() {
      this.reset();
      this.fallback(true);
    }
  }

  customElements.define("turnstile-widget", TurnstileWidget);
})();
