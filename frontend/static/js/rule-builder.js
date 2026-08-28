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

    function clone(templateId) {
        var template = document.getElementById(templateId);
        var fragment = template.content.cloneNode(true);
        var element = fragment.firstElementChild;
        // The row arrives with hx-on / live bindings that htmx has never seen.
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
            container().appendChild(clone('welcome-rule-blank'));
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
            list.appendChild(clone('welcome-condition-blank'));
            renumber();
        },

        removeCondition: function (button) {
            button.closest('.welcome-condition').remove();
            renumber();
        }
    };
})();
