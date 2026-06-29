// Reusable map-based region editor.
//
// Markup convention (rendered by templates/site/region_editor.stpl.html):
//
//   <div data-region-editor data-target="some-hidden-id">
//     <input type="hidden" id="some-hidden-id" name="regions" value="<existing GeoJSON>">
//     <div class="region-editor-map"></div>
//     <button class="region-editor-undo">…</button>
//     <button class="region-editor-clear">…</button>
//     <span class="region-editor-status"></span>
//   </div>
//
// Click the map to add vertices; 3+ vertices form a Polygon, a single vertex
// is stored as a Point. The chosen geometry is written to the hidden input as
// a GeoJSON string on every change. Loaded only when Head::with_region_editor
// is set, so `maplibregl` is guaranteed present.
(function () {
  if (typeof maplibregl === "undefined") return;

  var MAP_STYLE = "https://tiles.openfreemap.org/styles/liberty";
  var PRIMARY = "#037F9B";
  var ACCENT = "#F54017";
  var registry = [];

  function init(root) {
    if (root.__regionEditorInit) return;
    root.__regionEditorInit = true;

    var mapEl = root.querySelector(".region-editor-map");
    var targetId = root.getAttribute("data-target");
    var hidden = targetId
      ? document.getElementById(targetId)
      : root.querySelector('input[type="hidden"]');
    if (!mapEl || !hidden) return;

    var statusEl = root.querySelector(".region-editor-status");
    var points = parseInitial(hidden.value); // array of [lng, lat]

    var map = new maplibregl.Map({
      container: mapEl,
      style: MAP_STYLE,
      center: points.length ? points[0] : [-98.5, 39.8],
      zoom: points.length ? 6 : 3,
    });
    map.addControl(new maplibregl.NavigationControl({ showCompass: false }), "top-right");

    // The `regions` column is a MULTIPOLYGON, so a region is only meaningful
    // once it's an actual polygon (3+ points); fewer points store nothing.
    function geometry() {
      if (points.length < 3) return null;
      var ring = points.slice();
      ring.push(points[0]); // close the ring
      return { type: "MultiPolygon", coordinates: [[ring]] };
    }

    function fillData() {
      var g = geometry();
      return {
        type: "FeatureCollection",
        features: g ? [{ type: "Feature", geometry: g, properties: {} }] : [],
      };
    }
    function lineData() {
      if (points.length < 2) return { type: "FeatureCollection", features: [] };
      var coords = points.slice();
      if (points.length >= 3) coords.push(points[0]);
      return {
        type: "FeatureCollection",
        features: [{ type: "Feature", geometry: { type: "LineString", coordinates: coords }, properties: {} }],
      };
    }
    function vertexData() {
      return {
        type: "FeatureCollection",
        features: points.map(function (p) {
          return { type: "Feature", geometry: { type: "Point", coordinates: p }, properties: {} };
        }),
      };
    }

    function status() {
      if (!statusEl) return;
      if (points.length === 0) statusEl.textContent = "No area drawn";
      else if (points.length < 3)
        statusEl.textContent =
          points.length + " point" + (points.length === 1 ? "" : "s") + " — add " + (3 - points.length) + " more for a region";
      else statusEl.textContent = "Region outlined with " + points.length + " points";
    }

    function sync() {
      var g = geometry();
      hidden.value = g ? JSON.stringify(g) : "";
      if (map.getSource("region-fill")) map.getSource("region-fill").setData(fillData());
      if (map.getSource("region-line")) map.getSource("region-line").setData(lineData());
      if (map.getSource("region-verts")) map.getSource("region-verts").setData(vertexData());
      status();
    }

    map.on("load", function () {
      map.addSource("region-fill", { type: "geojson", data: fillData() });
      map.addLayer({ id: "region-fill", type: "fill", source: "region-fill", paint: { "fill-color": PRIMARY, "fill-opacity": 0.25 } });
      map.addSource("region-line", { type: "geojson", data: lineData() });
      map.addLayer({ id: "region-line", type: "line", source: "region-line", paint: { "line-color": PRIMARY, "line-width": 2 } });
      map.addSource("region-verts", { type: "geojson", data: vertexData() });
      map.addLayer({ id: "region-verts", type: "circle", source: "region-verts", paint: { "circle-radius": 5, "circle-color": ACCENT, "circle-stroke-width": 1.5, "circle-stroke-color": "#fff" } });

      if (points.length > 1) {
        var bounds = new maplibregl.LngLatBounds();
        points.forEach(function (p) { bounds.extend(p); });
        map.fitBounds(bounds, { padding: 40, maxZoom: 12, duration: 0 });
      }
      status();
    });

    map.on("click", function (e) {
      points.push([e.lngLat.lng, e.lngLat.lat]);
      sync();
    });

    var undo = root.querySelector(".region-editor-undo");
    if (undo) undo.addEventListener("click", function () { points.pop(); sync(); });
    var clear = root.querySelector(".region-editor-clear");
    if (clear) clear.addEventListener("click", function () { points = []; sync(); });

    registry.push({ root: root, map: map });
  }

  function parseInitial(value) {
    if (!value) return [];
    try {
      var g = JSON.parse(value);
      var ring = null;
      if (g && g.type === "MultiPolygon" && g.coordinates && g.coordinates[0] && g.coordinates[0][0]) {
        ring = g.coordinates[0][0].slice();
      } else if (g && g.type === "Polygon" && g.coordinates && g.coordinates[0]) {
        ring = g.coordinates[0].slice();
      }
      if (ring) {
        if (ring.length > 1) {
          var first = ring[0], last = ring[ring.length - 1];
          if (first[0] === last[0] && first[1] === last[1]) ring.pop();
        }
        return ring;
      }
    } catch (e) {}
    return [];
  }

  function initAll() {
    document.querySelectorAll("[data-region-editor]").forEach(init);
  }

  // Maps initialized while their container is hidden render at zero size;
  // call this after revealing one.
  function resizeAll() {
    registry.forEach(function (entry) {
      try { entry.map.resize(); } catch (e) {}
    });
  }

  window.RegionEditor = { init: init, initAll: initAll, resizeAll: resizeAll };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", initAll);
  } else {
    initAll();
  }
})();
