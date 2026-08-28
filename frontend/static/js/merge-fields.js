/*
 * Inserting a merge field into whichever of the email's two editors the author
 * last had the cursor in.
 *
 * The subject is a plain input and the body is a Quill instance, so there is no
 * single cursor to ask; this tracks which one was focused most recently and
 * inserts there. That is imperative editor work rather than DOM state, which is
 * why it lives here instead of in an hx-live binding.
 */
(function () {
    'use strict';

    var lastFocusedField = 'body';

    function trackSubject() {
        var subject = document.getElementById('subject');
        if (!subject || subject.dataset.mergeFieldTracked) return;
        subject.dataset.mergeFieldTracked = 'true';
        subject.addEventListener('focus', function () {
            lastFocusedField = 'subject';
        });
    }

    function trackEditor() {
        var element = document.querySelector('ss-rich-text');
        if (!element || element.dataset.mergeFieldTracked) return;

        // Quill is constructed asynchronously by the web component, so there is
        // nothing to subscribe to until it exists.
        var waiting = setInterval(function () {
            var quill = element.quill;
            if (!quill) return;
            clearInterval(waiting);
            element.dataset.mergeFieldTracked = 'true';
            quill.on('selection-change', function (range) {
                if (range) lastFocusedField = 'body';
            });
        }, 100);
    }

    function track() {
        trackSubject();
        trackEditor();
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', track);
    } else {
        track();
    }

    // The editors can arrive with a swap, so pick them up again afterwards.
    document.addEventListener('htmx:after:settle', track);

    window.insertMergeField = function (fieldName, event) {
        // The button lives inside a form; without this the click submits it.
        if (event) {
            event.preventDefault();
            event.stopPropagation();
        }

        var mergeTag = '{{' + fieldName + '}}';

        if (lastFocusedField === 'subject') {
            var subject = document.getElementById('subject');
            if (!subject) return;
            // Focus first, so the recorded selection is the current one.
            subject.focus();
            var start = subject.selectionStart;
            var end = subject.selectionEnd;
            subject.value =
                subject.value.substring(0, start) + mergeTag + subject.value.substring(end);
            subject.selectionStart = subject.selectionEnd = start + mergeTag.length;
            return;
        }

        var quill = document.querySelector('ss-rich-text') && document.querySelector('ss-rich-text').quill;
        if (!quill) return;
        quill.focus();
        var selection = quill.getSelection();
        var index = selection ? selection.index : 0;
        quill.insertText(index, mergeTag);
        quill.setSelection(index + mergeTag.length);
    };
})();
