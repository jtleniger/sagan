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
//     instant in UTC (what a reader without this file keeps).
//   * A filter boundary — an element whose `id` is `logs-from` or `logs-to` and whose
//     `data-utc` is the UTC instant the server rendered, holding `-date` and `-time` inputs
//     for the reader and a `-utc` input that is the only part of the boundary submitted.
(function () {
  "use strict";

  // The shape `logs.rs` writes and parses for a filter boundary: no seconds fraction, no
  // zone. Local values are formatted from the browser's clock fields, never from
  // `toLocaleString`, so the input always gets a string it can hold.
  function pad(value) {
    return (value < 10 ? "0" : "") + value;
  }

  function toLocalDateValue(date) {
    return date.getFullYear() + "-" + pad(date.getMonth() + 1) + "-" + pad(date.getDate());
  }

  function toLocalTimeValue(date) {
    return pad(date.getHours()) + ":" + pad(date.getMinutes()) + ":" + pad(date.getSeconds());
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

  // The reader's two controls and the hidden one they feed, per boundary. A boundary whose
  // parts are not all there is left alone rather than half-handled.
  function boundaries(form) {
    var groups = Array.prototype.slice.call(form.querySelectorAll("[data-utc]"));
    return groups
      .map(function (group) {
        return {
          utc: group.getAttribute("data-utc"),
          date: document.getElementById(group.id + "-date"),
          time: document.getElementById(group.id + "-time"),
          hidden: document.getElementById(group.id + "-utc")
        };
      })
      .filter(function (boundary) {
        return boundary.date && boundary.time && boundary.hidden;
      });
  }

  // Show a boundary on the reader's own clock instead of the UTC the server sent.
  function showLocal(boundary) {
    if (!boundary.utc) return;

    var instant = fromUtc(boundary.utc);
    if (!isValid(instant)) return;

    boundary.date.value = toLocalDateValue(instant);
    boundary.time.value = toLocalTimeValue(instant);
  }

  // Hand the boundary's instant to the query string in UTC. A boundary is a date and a time
  // together; with either one missing there is no instant to name, and the empty value the
  // hidden input then carries is what clears the filter.
  function submitUtc(boundary) {
    var instant =
      boundary.date.value && boundary.time.value
        ? new Date(boundary.date.value + "T" + boundary.time.value)
        : null;

    boundary.hidden.value = instant && isValid(instant) ? toUtc(instant) : "";
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

    var found = boundaries(form);
    found.forEach(showLocal);
    form.addEventListener("submit", function () {
      found.forEach(submitUtc);
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
