/*
 * Where a notification lands.
 *
 * A handler that answers an htmx request with a message doesn't know what part
 * of the page the request came from, so it doesn't choose a destination -- it
 * marks the response `X-Notification` and leaves placement here.
 *
 * The rule: a message goes to the nearest enclosing container that asked for
 * one, and to the page-wide region when nothing did. A container opts in with
 * `data-notifications`; giving the attribute a selector puts messages in a
 * particular element inside that container instead of the container itself.
 *
 *     <dialog data-notifications=".modal-message-container"> ... </dialog>
 *
 * This exists because a modal <dialog> renders in the browser's top layer,
 * above every z-index, so the fixed region is invisible behind an open modal --
 * exactly when an error is most likely and most needs reading.
 */
(function () {
    'use strict';

    var REGION = 'notifications';

    function destinationFor(source) {
        var owner = source && source.closest && source.closest('[data-notifications]');
        if (owner) {
            var selector = owner.dataset.notifications;
            // An empty attribute means "in here"; a selector names somewhere
            // more precise, and falls back to the container if it is missing.
            return (selector && owner.querySelector(selector)) || owner;
        }
        return document.getElementById(REGION);
    }

    document.addEventListener('htmx:after:request', function (event) {
        var ctx = event.detail && event.detail.ctx;
        if (!ctx || !ctx.response || !ctx.response.headers) return;
        if (!ctx.response.headers.get('X-Notification')) return;

        var destination = destinationFor(ctx.sourceElement);
        if (!destination) {
            // Nowhere to put it is not a reason to overwrite whatever the
            // request was targeting.
            ctx.swap = 'none';
            return;
        }

        ctx.target = destination;
        ctx.swap = 'beforeend';
    });
})();
