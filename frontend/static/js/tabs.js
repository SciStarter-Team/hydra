/* Generic tab groups.
 *
 * Markup contract (see components/tabs.css):
 *   .tabs-bar-wrapper
 *     [role=tablist]
 *       [role=tab][aria-controls=<panel-id>][aria-selected]
 *     ...
 *       [role=tabpanel][aria-hidden] siblings, ids referenced by the tabs
 *
 * Selecting a tab updates aria-selected within its own tablist and
 * aria-hidden among its target panel's siblings, so multiple tab groups
 * (including nested ones) can coexist on a page.
 *
 * A tablist may also contain an optional .chevron / .chev-btn pair that
 * hints at horizontally scrolled-out tabs on narrow screens.
 */
(function () {
    document.addEventListener('click', function (event) {
        const tab = event.target.closest('[role="tab"]');
        if (!tab || !tab.closest('.tabs-bar-wrapper')) return;

        const tablist = tab.closest('[role="tablist"]');
        const panel = document.getElementById(tab.getAttribute('aria-controls'));
        if (!tablist || !panel) return;

        tablist.querySelectorAll('[role="tab"]').forEach(function (t) {
            t.setAttribute('aria-selected', String(t === tab));
        });
        panel.parentElement
            .querySelectorAll(':scope > [role="tabpanel"]')
            .forEach(function (p) {
                p.setAttribute('aria-hidden', String(p !== panel));
            });
    });

    function initChevrons() {
        document.querySelectorAll('.tabs-bar-wrapper').forEach(function (wrapper) {
            const row = wrapper.querySelector('[role="tablist"]');
            const chevron = wrapper.querySelector('.chevron');
            if (!row || !chevron) return;

            function update() {
                const overflowRight =
                    row.scrollWidth - (row.scrollLeft + row.clientWidth);
                chevron.classList.toggle('visible', overflowRight > 2);
            }

            const chevBtn = wrapper.querySelector('.chev-btn');
            if (chevBtn) {
                chevBtn.addEventListener('click', function () {
                    row.scrollBy({ left: 120, behavior: 'smooth' });
                });
            }

            row.addEventListener('scroll', update);
            window.addEventListener('resize', update);
            update();
        });
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', initChevrons);
    } else {
        initChevrons();
    }
})();
