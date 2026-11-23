(function () {
  var MONTHS_SHORT = ['JAN', 'FEB', 'MAR', 'APR', 'MAY', 'JUN', 'JUL', 'AUG', 'SEP', 'OCT', 'NOV', 'DEC'];

  // How the formats that read as a moment are written; any of them can also
  // be written as a span.
  var OPTIONS = {
    'date': { year: 'numeric', month: 'long', day: 'numeric' },
    'short-date': { year: 'numeric', month: 'short', day: 'numeric' },
    'time': { hour: 'numeric', minute: '2-digit' },
    'datetime': { year: 'numeric', month: 'long', day: 'numeric', hour: 'numeric', minute: '2-digit' }
  };

  // The short name of the zone `tz` (the viewer's own when none is given)
  // at `at`, or now, as the event forms name zones whatever the viewer's
  // language: "PDT", "GMT+3". New York is EDT in July and EST in January.
  function zoneName(tz, at) {
    try {
      var options = { timeZoneName: 'short' };
      if (tz) options.timeZone = tz;
      var parts = new Intl.DateTimeFormat('en-US', options)
        .formatToParts(at ? new Date(at) : new Date());
      var name = parts.find(function (p) { return p.type === 'timeZoneName'; });
      return name ? name.value : (tz || '');
    } catch (e) {
      return tz || '';
    }
  }
  window.LocalTime = { zoneName: zoneName };

  // The options a format writes with on a node's clocks: a place's, named
  // by `data-zone`, or the viewer's own.
  function optionsFor(node, base) {
    var options = {};
    for (var key in base) options[key] = base[key];
    var zone = node.getAttribute('data-zone');
    if (zone) options.timeZone = zone;
    return options;
  }

  // The name of the node's clocks at `d`, when `data-named-zone` asks for it
  // and the format tells the time ("9:30 AM EST"); otherwise none.
  function namedZone(node, base, d) {
    if (!node.hasAttribute('data-named-zone') || !('hour' in base)) return '';
    return zoneName(node.getAttribute('data-zone'), d);
  }

  function named(text, zone) {
    return zone ? text + ' ' + zone : text;
  }

  // The year, month (1-12) and day of `d` on the node's clocks.
  function calendar(node, d) {
    var zone = node.getAttribute('data-zone');
    if (!zone) return { year: d.getFullYear(), month: d.getMonth() + 1, day: d.getDate() };
    var parts = {};
    new Intl.DateTimeFormat('en-US', { timeZone: zone, year: 'numeric', month: 'numeric', day: 'numeric' })
      .formatToParts(d)
      .forEach(function (p) { parts[p.type] = Number(p.value); });
    return parts;
  }

  function formatNode(node) {
    if (node.dataset.localTimeApplied === '1') return;
    var iso = node.getAttribute('datetime');
    if (!iso) return;
    var d = new Date(iso);
    if (isNaN(d.getTime())) return;
    var format = node.getAttribute('data-format');
    var text;

    try {
      // A span (`local_time_range`): formatRange writes what both ends share
      // only once, so a session reads "March 5, 2027, 9:30 – 11:00 PM EST".
      // A span across a change of the clocks names the zone at each end.
      if (node.hasAttribute('data-end') && OPTIONS[format]) {
        var end = new Date(node.getAttribute('data-end'));
        if (isNaN(end.getTime())) return;
        var range = new Intl.DateTimeFormat(undefined, optionsFor(node, OPTIONS[format]));
        var from = namedZone(node, OPTIONS[format], d);
        var to = namedZone(node, OPTIONS[format], end);
        if (from === to && range.formatRange) {
          node.textContent = named(range.formatRange(d, end), from);
        } else {
          node.textContent = named(range.format(d), from) + ' \u2013 ' + named(range.format(end), to);
        }
        node.dataset.localTimeApplied = '1';
        return;
      }

      if (OPTIONS[format]) {
        text = named(
          new Intl.DateTimeFormat(undefined, optionsFor(node, OPTIONS[format])).format(d),
          namedZone(node, OPTIONS[format], d)
        );
      } else {
        var day = calendar(node, d);
        switch (format) {
          case 'month':
            text = MONTHS_SHORT[day.month - 1];
            break;
          case 'day':
            text = String(day.day);
            break;
          case 'year':
            text = String(day.year);
            break;
          default:
            return;
        }
      }
    } catch (e) {
      // A zone the browser doesn't know: the server's text stands.
      return;
    }
    node.textContent = text;
    node.dataset.localTimeApplied = '1';
  }

  function convertTimes(root) {
    var scope = root && root.querySelectorAll ? root : document;
    var nodes = scope.querySelectorAll('time[datetime][data-format]');
    for (var i = 0; i < nodes.length; i++) formatNode(nodes[i]);
    if (root && root.nodeType === 1 && root.matches && root.matches('time[datetime][data-format]')) {
      formatNode(root);
    }
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', function () { convertTimes(); });
  } else {
    convertTimes();
  }
  document.addEventListener('htmx:after:settle', function (e) { convertTimes(e.target); });
  // An outerHTML swap's settle event goes to the element it replaced, no
  // longer in the page; each new root is processed once it's in.
  document.addEventListener('htmx:after:process', function (e) { convertTimes(e.target); });
})();
