// Render every `time[data-local-time]` in the reader's own zone.
//
// The backend stores, filters and renders every timestamp in UTC; a reader is not in UTC.
// This file is the one place that conversion happens, loaded by every page that renders a
// timestamp (see docs/templates.md).
//
// One contract with the markup:
//
//   * `time[data-local-time]` — `datetime` is the RFC 3339 instant, the text is the same
//     instant in UTC (what a reader without this file keeps).
(function () {
  "use strict";

  function isValid(date) {
    return !isNaN(date.getTime());
  }

  function showLocalTimes() {
    // Explicit fields rather than `dateStyle`/`timeStyle`, which cannot carry the
    // milliseconds a log record is read for.
    var formatter = new Intl.DateTimeFormat(undefined, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      fractionalSecondDigits: 3
    });

    var rows = document.querySelectorAll("time[data-local-time]");
    for (var i = 0; i < rows.length; i++) {
      var date = new Date(rows[i].getAttribute("datetime"));
      if (!isValid(date)) continue;

      rows[i].textContent = formatter.format(date);
      // The UTC instant stays one hover away.
      rows[i].title = date.toISOString();
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", showLocalTimes);
  } else {
    showLocalTimes();
  }
})();
