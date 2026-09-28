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

  // Explicit fields rather than `dateStyle`/`timeStyle`. Two formatters: an instant that
  // carries milliseconds keeps them (a log record is read for them), an instant that does
  // not stays second-precision — so the reader-facing text never gains precision the UTC
  // fallback it replaces did not have.
  var SECOND = new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit"
  });

  var MILLIS = new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    fractionalSecondDigits: 3
  });

  function showLocalTimes() {
    var rows = document.querySelectorAll("time[data-local-time]");
    for (var i = 0; i < rows.length; i++) {
      var datetime = rows[i].getAttribute("datetime");
      var date = new Date(datetime);
      if (!isValid(date)) continue;

      var formatter = datetime.indexOf(".") === -1 ? SECOND : MILLIS;
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

  // htmx inserts fresh `time[data-local-time]` elements (the front page's Live card, every
  // 15 s). The function is idempotent — it re-derives the text from `datetime` — so running it
  // again after a swap is safe.
  document.body.addEventListener("htmx:afterSwap", showLocalTimes);
})();
