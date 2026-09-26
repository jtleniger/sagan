// Local time for the Logs page.
//
// The backend stores, filters and renders every timestamp in UTC; a reader is not in UTC.
// This file converts what the server sent into the reader's own zone, and converts what
// the reader typed back into UTC before the filter form is submitted — so the query string
// the backend parses is UTC either way.
//
// Two contracts with `assets/views/logs/index.html`:
//
//   * `time[data-local-time]` — `datetime` is the RFC 3339 instant, the text is the same
//     instant in UTC (the fallback for a reader without JavaScript).
//   * `input[data-utc]` — a `datetime-local` filter boundary, `data-utc` being the UTC value
//     the server rendered. Its `id` names its hidden twin (`-utc`), which is disabled until
//     this file enables it with the UTC translation of what the reader typed.
(function () {
  "use strict";

  // The shape `logs.rs` writes and parses for a filter boundary: no seconds fraction, no
  // zone. Local values are formatted from the browser's clock fields, never from
  // `toLocaleString`, so the input always gets a string it can hold.
  function pad(value) {
    return (value < 10 ? "0" : "") + value;
  }

  function toLocalInputValue(date) {
    return (
      date.getFullYear() +
      "-" +
      pad(date.getMonth() + 1) +
      "-" +
      pad(date.getDate()) +
      "T" +
      pad(date.getHours()) +
      ":" +
      pad(date.getMinutes()) +
      ":" +
      pad(date.getSeconds())
    );
  }

  // A `Date` parses an unzoned date-time string as local time, so the UTC this file reads
  // from the server has to say so. `toISOString` is UTC by definition; its first 19
  // characters are exactly the value the backend parses.
  function fromUtc(value) {
    return new Date(value + "Z");
  }

  function toUtc(value) {
    return value.toISOString().slice(0, 19);
  }

  function isValid(date) {
    return !isNaN(date.getTime());
  }

  function boundaries(form) {
    return Array.prototype.slice.call(form.querySelectorAll("input[data-utc]"));
  }

  // Show a boundary in the reader's zone instead of the UTC the server sent.
  function showLocal(input) {
    var utc = input.getAttribute("data-utc");
    if (!utc) return;

    var date = fromUtc(utc);
    if (isValid(date)) input.value = toLocalInputValue(date);
  }

  // Hand the boundary's instant to the query string in UTC, and stop the input the reader
  // edited from being submitted next to it.
  function submitUtc(input) {
    var hidden = document.getElementById(input.id + "-utc");
    if (!hidden) return;

    var date = new Date(input.value);
    var utc = input.value && isValid(date) ? toUtc(date) : "";

    hidden.value = utc;
    // An unset boundary is an absent query parameter, not an empty one.
    hidden.disabled = utc === "";
    input.removeAttribute("name");
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

  function init() {
    showLocalTimes();

    var form = document.querySelector("form[data-logs-filter]");
    if (!form) return;

    boundaries(form).forEach(showLocal);
    form.addEventListener("submit", function () {
      boundaries(form).forEach(submitUtc);
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
