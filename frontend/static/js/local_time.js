(function () {
  var MONTHS_SHORT = ['JAN', 'FEB', 'MAR', 'APR', 'MAY', 'JUN', 'JUL', 'AUG', 'SEP', 'OCT', 'NOV', 'DEC'];

  function formatNode(node) {
    if (node.dataset.localTimeApplied === '1') return;
    var iso = node.getAttribute('datetime');
    if (!iso) return;
    var d = new Date(iso);
    if (isNaN(d.getTime())) return;
    var format = node.getAttribute('data-format');
    var text;
    switch (format) {
      case 'date':
        text = d.toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' });
        break;
      case 'short-date':
        text = d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
        break;
      case 'time':
        text = d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
        break;
      case 'datetime':
        text = d.toLocaleString(undefined, { year: 'numeric', month: 'long', day: 'numeric', hour: 'numeric', minute: '2-digit' });
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
  document.addEventListener('htmx:afterSettle', function (e) { convertTimes(e.target); });
})();
