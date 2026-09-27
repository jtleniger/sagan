// The Logs page's time filter.
//
// The backend stores, filters and renders every timestamp in UTC; a reader is not in UTC.
// This file converts what the reader typed back into UTC before the filter form is
// submitted, so the query string the backend parses is UTC either way. Rendering the
// server's UTC instants in the reader's zone is `local-time.js`, which this page loads too.
//
// One contract with `assets/views/logs/index.html`:
//
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

  function init() {
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
