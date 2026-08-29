/*
 * Adding, removing and reordering the welcome rules on the dashboard settings
 * page.
 *
 * The rows themselves are ordinary form fields, so their values need no
 * shadowing here -- the inputs are the state, and which extra inputs a
 * condition shows is an hx-live binding on the row. What is left is the one
 * thing bindings cannot express: creating and destroying rows, and keeping the
 * `rules[i][conditions][j][...]` indices contiguous so serde_qs can read them
 * back as lists.
 *
 * Every field carries `data-name`, its name with `__RI__`/`__CI__` still in it.
 * Renumbering rewrites `name` from that pattern rather than parsing the old
 * name, so the field names have exactly one source: the template.
 */
window.ruleBuilder = (function () {
    'use strict';

    function container() {
        return document.getElementById('welcome-rules');
    }

    /* Clones a blank row into `parent`.
     *
     * The order matters: htmx.process() returns immediately for anything not
     * in the document, so a row processed before it is appended keeps none of
     * its hx-on handlers or live bindings -- its buttons look right and do
     * nothing at all. Append first, then process. */
    function addRow(parent, templateId) {
        var template = document.getElementById(templateId);
        var element = template.content.cloneNode(true).firstElementChild;
        parent.appendChild(element);
        htmx.process(element);
        return element;
    }

    /** Rewrites every field name so the indices match the rows' positions. */
    function renumber() {
        var rules = container().querySelectorAll('.welcome-rule');
        rules.forEach(function (rule, ri) {
            var number = rule.querySelector('.rule-number');
            if (number) number.textContent = String(ri + 1);

            var conditions = rule.querySelectorAll('.welcome-condition');
            rule.querySelectorAll('[data-name]').forEach(function (field) {
                // A condition's fields are numbered by their own row, so they
                // are handled with that row below.
                if (field.closest('.welcome-condition')) return;
                field.name = field.dataset.name.replace('__RI__', ri);
            });

            conditions.forEach(function (condition, ci) {
                condition.querySelectorAll('[data-name]').forEach(function (field) {
                    field.name = field.dataset.name
                        .replace('__RI__', ri)
                        .replace('__CI__', ci);
                });
            });
        });
        // Row removal can change which inputs a condition should show.
        htmx.live.refresh();
    }

    return {
        addRule: function () {
            addRow(container(), 'welcome-rule-blank');
            renumber();
        },

        removeRule: function (button) {
            button.closest('.welcome-rule').remove();
            renumber();
        },

        moveRule: function (button, direction) {
            var rule = button.closest('.welcome-rule');
            var sibling = direction < 0
                ? rule.previousElementSibling
                : rule.nextElementSibling;
            if (!sibling) return;
            if (direction < 0) rule.parentNode.insertBefore(rule, sibling);
            else rule.parentNode.insertBefore(sibling, rule);
            renumber();
        },

        addCondition: function (button) {
            var list = button.closest('.welcome-rule').querySelector('.welcome-conditions');
            addRow(list, 'welcome-condition-blank');
            renumber();
        },

        removeCondition: function (button) {
            button.closest('.welcome-condition').remove();
            renumber();
        }
    };
})();
