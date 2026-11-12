/* A hosted form's page (templates/hosted_form/).
 *
 * Nothing here talks to the server but the map, which reads its markers'
 * GeoJSON: the form is sent as a plain form, and every check is the
 * server's. What this does is all on the page:
 *
 * - A field shown by another's answer (fieldset[data-show-if-name]) is shown
 *   while that one is shown and answered so, and is otherwise hidden and
 *   disabled, so nothing of it is sent. The server decides the same way
 *   (page.rs `visibility`); a field depends only on fields before it, so one
 *   pass in order settles them all.
 * - A place field ([data-location-field]) is a "latitude, longitude" box,
 *   with a map to pick the place on.
 * - The map tab ([data-hosted-form-map]) is drawn when it is first shown,
 *   its markers clustered, each opening the popup the server drew for it.
 * - A button [data-show-tab=<tab id>] selects that tab.
 */
(function () {
    'use strict';

    const MAP_STYLE = 'https://tiles.openfreemap.org/styles/liberty';

    // ---------------------------------------------------------------- shown

    /* What the controls named `name` answer, as far as showing a field goes:
     * the values of those ticked or chosen, a box left unticked answering
     * its data-unchecked value. */
    function answered(form, name) {
        const values = [];

        for (const control of form.elements) {
            if (control.name !== name || control.matches(':disabled')) continue;

            if (control.type === 'checkbox' || control.type === 'radio') {
                if (control.checked) {
                    values.push(control.value);
                } else if (control.dataset.unchecked !== undefined) {
                    values.push(control.dataset.unchecked);
                }
            } else if (control.tagName === 'SELECT') {
                for (const option of control.selectedOptions) values.push(option.value);
            } else {
                values.push(control.value.trim());
            }
        }

        return values;
    }

    function settle(form) {
        form.querySelectorAll('fieldset.hf-field[data-show-if-name]').forEach(function (field) {
            const shown = answered(form, field.dataset.showIfName)
                .includes(field.dataset.showIfValue);

            field.hidden = !shown;
            field.disabled = !shown;
        });
    }

    function initConditions(form) {
        if (!form.querySelector('fieldset.hf-field[data-show-if-name]')) return;

        form.addEventListener('change', function () { settle(form); });
        form.addEventListener('input', function () { settle(form); });
        settle(form);
    }

    // ---------------------------------------------------------------- place

    function parsePlace(text) {
        const parts = text.split(',').map(function (part) { return Number(part.trim()); });
        if (parts.length !== 2 || parts.some(function (n) { return !Number.isFinite(n); })) {
            return null;
        }

        const [latitude, longitude] = parts;
        if (Math.abs(latitude) > 90 || Math.abs(longitude) > 180) return null;

        return { latitude: latitude, longitude: longitude };
    }

    function writePlace(input, latitude, longitude) {
        input.value = latitude.toFixed(5) + ', ' + longitude.toFixed(5);
        input.dispatchEvent(new Event('change', { bubbles: true }));
    }

    function initLocation(root) {
        const input = root.querySelector('.hf-location-input');
        const mapBox = root.querySelector('.hf-location-map');
        const hint = root.querySelector('.hf-location-hint');
        const here = root.querySelector('.hf-location-here');
        if (!input || !mapBox || typeof maplibregl === 'undefined') return;

        let map = null;
        let marker = null;

        function place(latitude, longitude) {
            const at = [longitude, latitude];
            if (marker) {
                marker.setLngLat(at);
            } else {
                marker = new maplibregl.Marker({ draggable: true }).setLngLat(at).addTo(map);
                marker.on('dragend', function () {
                    const moved = marker.getLngLat();
                    writePlace(input, moved.lat, moved.lng);
                });
            }
        }

        function open() {
            if (map) return;

            mapBox.hidden = false;
            hint.hidden = false;

            const given = parsePlace(input.value);
            const center = given || (root.dataset.center ? parsePlace(root.dataset.center) : null);

            map = new maplibregl.Map({
                container: mapBox,
                style: MAP_STYLE,
                center: center ? [center.longitude, center.latitude] : [0, 20],
                zoom: given ? 13 : center ? 9 : 1,
            });
            map.addControl(new maplibregl.NavigationControl({ showCompass: false }));

            // No marker until a place is picked: the map's centre is only a
            // guess, and is not an answer.
            if (given) place(given.latitude, given.longitude);

            map.on('click', function (event) {
                place(event.lngLat.lat, event.lngLat.lng);
                writePlace(input, event.lngLat.lat, event.lngLat.lng);
            });
        }

        input.addEventListener('focus', open);
        input.addEventListener('change', function () {
            const given = parsePlace(input.value);
            if (map && given) {
                place(given.latitude, given.longitude);
                map.easeTo({ center: [given.longitude, given.latitude] });
            }
        });

        if (here && 'geolocation' in navigator) {
            here.hidden = false;
            here.addEventListener('click', function () {
                open();
                navigator.geolocation.getCurrentPosition(function (position) {
                    const { latitude, longitude } = position.coords;
                    place(latitude, longitude);
                    writePlace(input, latitude, longitude);
                    map.easeTo({ center: [longitude, latitude], zoom: 14 });
                });
            });
        }
    }

    // ---------------------------------------------------------------- map

    function drawMap(box) {
        if (box.dataset.drawn || typeof maplibregl === 'undefined') return;
        box.dataset.drawn = 'true';

        const empty = box.querySelector('.hf-map-empty');
        const map = new maplibregl.Map({
            container: box,
            style: MAP_STYLE,
            center: [-96, 38],
            zoom: 2,
        });
        map.addControl(new maplibregl.NavigationControl({ showCompass: false }));

        map.on('load', function () {
            fetch(box.dataset.hostedFormMap, { credentials: 'same-origin' })
                .then(function (response) {
                    if (!response.ok) throw new Error('map ' + response.status);
                    return response.json();
                })
                .then(function (markers) {
                    if (markers.features.length === 0) {
                        if (empty) empty.hidden = false;
                        return;
                    }

                    map.addSource('submissions', {
                        type: 'geojson',
                        data: markers,
                        cluster: true,
                        clusterRadius: 40,
                        clusterMaxZoom: 14,
                    });
                    map.addLayer({
                        id: 'clusters',
                        type: 'circle',
                        source: 'submissions',
                        filter: ['has', 'point_count'],
                        paint: {
                            'circle-color': '#0b6e75',
                            'circle-opacity': 0.85,
                            'circle-radius': ['step', ['get', 'point_count'], 14, 25, 18, 250, 24],
                            'circle-stroke-width': 2,
                            'circle-stroke-color': '#ffffff',
                        },
                    });
                    map.addLayer({
                        id: 'cluster-count',
                        type: 'symbol',
                        source: 'submissions',
                        filter: ['has', 'point_count'],
                        layout: {
                            'text-field': ['get', 'point_count_abbreviated'],
                            'text-size': 12,
                        },
                        paint: { 'text-color': '#ffffff' },
                    });
                    map.addLayer({
                        id: 'submission',
                        type: 'circle',
                        source: 'submissions',
                        filter: ['!', ['has', 'point_count']],
                        paint: {
                            'circle-color': '#e8604c',
                            'circle-radius': 7,
                            'circle-stroke-width': 2,
                            'circle-stroke-color': '#ffffff',
                        },
                    });

                    const bounds = new maplibregl.LngLatBounds();
                    markers.features.forEach(function (feature) {
                        bounds.extend(feature.geometry.coordinates);
                    });
                    map.fitBounds(bounds, { padding: 40, maxZoom: 12, duration: 0 });

                    map.on('click', 'clusters', function (event) {
                        const cluster = event.features[0];
                        map.getSource('submissions')
                            .getClusterExpansionZoom(cluster.properties.cluster_id)
                            .then(function (zoom) {
                                map.easeTo({ center: cluster.geometry.coordinates, zoom: zoom });
                            });
                    });

                    // The popup's HTML is the server's, its text escaped there.
                    map.on('click', 'submission', function (event) {
                        const feature = event.features[0];
                        new maplibregl.Popup({ maxWidth: '320px' })
                            .setLngLat(feature.geometry.coordinates)
                            .setHTML(feature.properties.popup)
                            .addTo(map);
                    });

                    ['clusters', 'submission'].forEach(function (layer) {
                        map.on('mouseenter', layer, function () { map.getCanvas().style.cursor = 'pointer'; });
                        map.on('mouseleave', layer, function () { map.getCanvas().style.cursor = ''; });
                    });
                })
                .catch(function (error) {
                    console.error('Hosted form map:', error);
                    if (empty) {
                        empty.textContent = 'The map could not be loaded.';
                        empty.hidden = false;
                    }
                });
        });
    }

    function initMap(box) {
        const panel = box.closest('[role="tabpanel"]');

        if (!panel || panel.getAttribute('aria-hidden') !== 'true') {
            drawMap(box);
            return;
        }

        // A map drawn in a hidden panel has no size; draw it once shown.
        const watch = new MutationObserver(function () {
            if (panel.getAttribute('aria-hidden') === 'false') {
                watch.disconnect();
                drawMap(box);
            }
        });
        watch.observe(panel, { attributes: true, attributeFilter: ['aria-hidden'] });
    }

    // ---------------------------------------------------------------- tabs

    document.addEventListener('click', function (event) {
        const button = event.target.closest('[data-show-tab]');
        if (!button) return;

        const tab = document.getElementById(button.dataset.showTab);
        if (tab) {
            tab.click();
            tab.scrollIntoView({ behavior: 'smooth', block: 'start' });
        }
    });

    function init() {
        document.querySelectorAll('form.hf-form').forEach(initConditions);
        document.querySelectorAll('[data-location-field]').forEach(initLocation);
        document.querySelectorAll('[data-hosted-form-map]').forEach(initMap);
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }
})();
