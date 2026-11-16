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

  // The options a format writes with on a node's clocks: a place's, named
  // by `data-zone`, or the viewer's own; `data-named-zone` names the zone in
  // the text ("9:30 AM EST").
  function optionsFor(node, base) {
    var options = {};
    for (var key in base) options[key] = base[key];
    var zone = node.getAttribute('data-zone');
    if (zone) options.timeZone = zone;
    if (node.hasAttribute('data-named-zone') && 'hour' in base) options.timeZoneName = 'short';
    return options;
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
      // only once, so a session reads "March 5, 2027, 9:30 – 11:00 PM".
      if (node.hasAttribute('data-end') && OPTIONS[format]) {
        var end = new Date(node.getAttribute('data-end'));
        if (isNaN(end.getTime())) return;
        var range = new Intl.DateTimeFormat(undefined, optionsFor(node, OPTIONS[format]));
        node.textContent = range.formatRange
          ? range.formatRange(d, end)
          : range.format(d) + ' \u2013 ' + range.format(end);
        node.dataset.localTimeApplied = '1';
        return;
      }

      if (OPTIONS[format]) {
        text = new Intl.DateTimeFormat(undefined, optionsFor(node, OPTIONS[format])).format(d);
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
