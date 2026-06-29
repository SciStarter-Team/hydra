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
// Click the map to add vertices to the current polygon; shift-click starts a
// new polygon. 3+ vertices form a polygon; the geometry is written to the
// hidden input as a GeoJSON **MultiPolygon** (matching the `regions` column).
// Loaded only when Head::with_region_editor is set, so `maplibregl` is present.
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
    // An array of rings; each ring is an array of [lng, lat]. The last ring is
    // the one currently being drawn. Always keep at least one (possibly empty).
    var rings = parseInitial(hidden.value);
    if (rings.length === 0) rings = [[]];

    function firstPoint() {
      for (var i = 0; i < rings.length; i++) {
        if (rings[i].length) return rings[i][0];
      }
      return null;
    }

    var start = firstPoint();
    var map = new maplibregl.Map({
      container: mapEl,
      style: MAP_STYLE,
      center: start || [-98.5, 39.8],
      zoom: start ? 6 : 3,
    });
    map.addControl(new maplibregl.NavigationControl({ showCompass: false }), "top-right");
    // Free up shift-click (used to start a new polygon) from the default
    // shift-drag box-zoom interaction.
    map.boxZoom.disable();

    function validRings() {
      return rings.filter(function (r) { return r.length >= 3; });
    }

    // GeoJSON MultiPolygon of all rings with 3+ points (each closed), else null.
    function geometry() {
      var valid = validRings();
      if (valid.length === 0) return null;
      return {
        type: "MultiPolygon",
        coordinates: valid.map(function (r) {
          var ring = r.slice();
          ring.push(r[0]); // close
          return [ring];
        }),
      };
    }

    function fillData() {
      var g = geometry();
      return {
        type: "FeatureCollection",
        features: g ? [{ type: "Feature", geometry: g, properties: {} }] : [],
      };
    }
    function lineData() {
      var features = [];
      rings.forEach(function (r) {
        if (r.length < 2) return;
        var coords = r.slice();
        if (r.length >= 3) coords.push(r[0]);
        features.push({ type: "Feature", geometry: { type: "LineString", coordinates: coords }, properties: {} });
      });
      return { type: "FeatureCollection", features: features };
    }
    function vertexData() {
      var features = [];
      rings.forEach(function (r, ri) {
        r.forEach(function (p, pi) {
          features.push({ type: "Feature", geometry: { type: "Point", coordinates: p }, properties: { r: ri, p: pi } });
        });
      });
      return { type: "FeatureCollection", features: features };
    }

    function status() {
      if (!statusEl) return;
      var valid = validRings().length;
      var pending = rings[rings.length - 1].length;
      if (valid === 0 && pending === 0) {
        statusEl.textContent = "No area drawn";
      } else if (valid === 0) {
        statusEl.textContent = pending + " point" + (pending === 1 ? "" : "s") + " — add " + (3 - pending) + " more for a region";
      } else {
        statusEl.textContent = valid + " area" + (valid === 1 ? "" : "s") + " drawn — shift-click to start another";
      }
    }

    function sync() {
      var g = geometry();
      hidden.value = g ? JSON.stringify(g) : "";
      if (map.getSource("region-fill")) map.getSource("region-fill").setData(fillData());
      if (map.getSource("region-line")) map.getSource("region-line").setData(lineData());
      if (map.getSource("region-verts")) map.getSource("region-verts").setData(vertexData());
      status();
    }

    function fitToRings() {
      var pts = [];
      rings.forEach(function (r) { pts = pts.concat(r); });
      if (pts.length > 1) {
        var bounds = new maplibregl.LngLatBounds();
        pts.forEach(function (p) { bounds.extend(p); });
        map.fitBounds(bounds, { padding: 40, maxZoom: 12, duration: 0 });
      }
    }

    // Replace the drawn area with a new set of rings (used by the place lookup).
    function loadRings(rs) {
      if (!rs.length) return false;
      rings = rs;
      sync();
      fitToRings();
      return true;
    }

    map.on("load", function () {
      map.addSource("region-fill", { type: "geojson", data: fillData() });
      map.addLayer({ id: "region-fill", type: "fill", source: "region-fill", paint: { "fill-color": PRIMARY, "fill-opacity": 0.25 } });
      map.addSource("region-line", { type: "geojson", data: lineData() });
      map.addLayer({ id: "region-line", type: "line", source: "region-line", paint: { "line-color": PRIMARY, "line-width": 2 } });
      map.addSource("region-verts", { type: "geojson", data: vertexData() });
      map.addLayer({ id: "region-verts", type: "circle", source: "region-verts", paint: { "circle-radius": 5, "circle-color": ACCENT, "circle-stroke-width": 1.5, "circle-stroke-color": "#fff" } });

      fitToRings();
      status();
    });

    // --- Vertex editing -----------------------------------------------------
    // Click empty map to add a point; click an existing vertex to delete it;
    // drag a vertex to move it. Shift-click empty map starts a new polygon.
    var dragging = null; // { r, p } while moving a vertex
    var moved = false; // did the current drag actually move
    var lastVertexDragEnd = 0; // timestamp, to ignore a click right after a drag

    function deleteVertex(r, p) {
      if (!rings[r]) return;
      rings[r].splice(p, 1);
      if (rings[r].length === 0 && rings.length > 1) rings.splice(r, 1);
      if (rings.length === 0) rings = [[]];
      sync();
    }

    map.on("click", function (e) {
      if (Date.now() - lastVertexDragEnd < 250) return; // ignore the click ending a drag
      var box = [[e.point.x - 6, e.point.y - 6], [e.point.x + 6, e.point.y + 6]];
      var hits = map.queryRenderedFeatures(box, { layers: ["region-verts"] });
      if (hits.length) {
        deleteVertex(Number(hits[0].properties.r), Number(hits[0].properties.p));
        return;
      }
      var shift = e.originalEvent && e.originalEvent.shiftKey;
      if (shift && rings[rings.length - 1].length > 0) rings.push([]);
      rings[rings.length - 1].push([e.lngLat.lng, e.lngLat.lat]);
      sync();
    });

    function onVertexDragMove(e) {
      if (!dragging || !rings[dragging.r]) return;
      moved = true;
      rings[dragging.r][dragging.p] = [e.lngLat.lng, e.lngLat.lat];
      sync();
    }
    function onVertexDragEnd() {
      map.off("mousemove", onVertexDragMove);
      map.getCanvas().style.cursor = "";
      if (moved) lastVertexDragEnd = Date.now(); // don't treat the release as a delete
      dragging = null;
    }
    map.on("mousedown", "region-verts", function (e) {
      if (!e.features || !e.features.length) return;
      e.preventDefault(); // cancel map drag-pan for this gesture
      dragging = { r: Number(e.features[0].properties.r), p: Number(e.features[0].properties.p) };
      moved = false;
      map.getCanvas().style.cursor = "grabbing";
      map.on("mousemove", onVertexDragMove);
      map.once("mouseup", onVertexDragEnd);
    });
    map.on("mouseenter", "region-verts", function () { if (!dragging) map.getCanvas().style.cursor = "move"; });
    map.on("mouseleave", "region-verts", function () { if (!dragging) map.getCanvas().style.cursor = ""; });

    var undo = root.querySelector(".region-editor-undo");
    if (undo) undo.addEventListener("click", function () {
      var last = rings[rings.length - 1];
      last.pop();
      // If the current ring is now empty and it isn't the only one, drop it so
      // a subsequent undo affects the previous polygon.
      if (last.length === 0 && rings.length > 1) rings.pop();
      sync();
    });
    var clear = root.querySelector(".region-editor-clear");
    if (clear) clear.addEventListener("click", function () { rings = [[]]; sync(); });

    // Alternate input: type a place name (city/state/country) and load its
    // simplified boundary polygon from the server (OSM Nominatim).
    var lookupInput = root.querySelector(".region-lookup-input");
    var lookupBtn = root.querySelector(".region-lookup-btn");
    var lookupStatus = root.querySelector(".region-lookup-status");
    function doLookup() {
      if (!lookupInput) return;
      var q = (lookupInput.value || "").trim();
      if (!q) return;
      if (lookupStatus) lookupStatus.textContent = "Searching…";
      fetch("/add/region-lookup?q=" + encodeURIComponent(q), { headers: { Accept: "application/json" } })
        .then(function (r) { return r.ok ? r.json() : null; })
        .then(function (g) {
          if (g && g.coordinates && loadRings(geometryToRings(g))) {
            if (lookupStatus) lookupStatus.textContent = "Loaded — Clear to start over, or shift-click to add another area.";
          } else if (lookupStatus) {
            lookupStatus.textContent = "No matching place found.";
          }
        })
        .catch(function () { if (lookupStatus) lookupStatus.textContent = "Lookup failed."; });
    }
    if (lookupBtn) lookupBtn.addEventListener("click", doLookup);
    if (lookupInput) {
      lookupInput.addEventListener("keydown", function (e) {
        if (e.key === "Enter") { e.preventDefault(); doLookup(); }
      });
    }

    registry.push({ root: root, map: map });
  }

  // Parse existing GeoJSON into an array of rings (outer ring per polygon).
  // Convert a GeoJSON Polygon/MultiPolygon geometry into our array of outer rings.
  function geometryToRings(g) {
    if (g && g.type === "MultiPolygon" && g.coordinates) {
      return g.coordinates
        .map(function (poly) { return openRing(poly && poly[0]); })
        .filter(function (r) { return r && r.length; });
    }
    if (g && g.type === "Polygon" && g.coordinates) {
      var r = openRing(g.coordinates[0]);
      return r && r.length ? [r] : [];
    }
    return [];
  }

  function parseInitial(value) {
    if (!value) return [];
    try {
      return geometryToRings(JSON.parse(value));
    } catch (e) {
      return [];
    }
  }

  // Drop a ring's closing duplicate point if present.
  function openRing(ring) {
    if (!ring || !ring.length) return [];
    var out = ring.slice();
    if (out.length > 1) {
      var first = out[0], last = out[out.length - 1];
      if (first[0] === last[0] && first[1] === last[1]) out.pop();
    }
    return out;
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
