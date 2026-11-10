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

  function formatNode(node) {
    if (node.dataset.localTimeApplied === '1') return;
    var iso = node.getAttribute('datetime');
    if (!iso) return;
    var d = new Date(iso);
    if (isNaN(d.getTime())) return;
    var format = node.getAttribute('data-format');
    var text;

    // A span (`local_time_range`): formatRange writes what both ends share
    // only once, so a session reads "March 5, 2027, 9:30 – 11:00 PM".
    if (node.hasAttribute('data-end') && OPTIONS[format]) {
      var end = new Date(node.getAttribute('data-end'));
      if (isNaN(end.getTime())) return;
      var range = new Intl.DateTimeFormat(undefined, OPTIONS[format]);
      node.textContent = range.formatRange
        ? range.formatRange(d, end)
        : range.format(d) + ' \u2013 ' + range.format(end);
      node.dataset.localTimeApplied = '1';
      return;
    }

    switch (format) {
      case 'date':
        text = d.toLocaleDateString(undefined, OPTIONS['date']);
        break;
      case 'short-date':
        text = d.toLocaleDateString(undefined, OPTIONS['short-date']);
        break;
      case 'time':
        text = d.toLocaleTimeString(undefined, OPTIONS['time']);
        break;
      case 'datetime':
        text = d.toLocaleString(undefined, OPTIONS['datetime']);
        break;
      case 'month':
        text = MONTHS_SHORT[d.getMonth()];
        break;
      case 'day':
        text = String(d.getDate());
        break;
      case 'year':
        text = String(d.getFullYear());
        break;
      default:
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
})();
