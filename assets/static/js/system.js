// Live charts for the System page.
//
// htmx polls: every 2 s it replaces `#system-live` (assets/views/system/_metrics.html) with a
// server-rendered fragment carrying the current sample as JSON in `data-sample`. Samples are
// appended here; the oldest are dropped, so the charts show a sliding window.
(function () {
  "use strict";

  // 2 s per sample (the hx-trigger on #system-live) × 150 = 5 minutes.
  var MAX_POINTS = 150;
  var CPU_COLOR = "#0f172a";
  var MEM_COLOR = "#2563eb";
  var TEMP_COLORS = ["#dc2626", "#ea580c", "#ca8a04", "#16a34a", "#0891b2", "#7c3aed", "#db2777", "#64748b"];

  var charts = null;

  function readSample() {
    var el = document.getElementById("system-live");
    if (!el || !el.dataset.sample) return null;
    try {
      return JSON.parse(el.dataset.sample);
    } catch (err) {
      return null;
    }
  }

  function push(series, x, y) {
    series.push({ x: x, y: y });
    while (series.length > MAX_POINTS) series.shift();
  }

  function lineOptions(yMin, yMax, unit, showLegend) {
    return {
      animation: false,
      responsive: true,
      maintainAspectRatio: false,
      parsing: false,
      elements: { point: { radius: 0 }, line: { borderWidth: 2, tension: 0 } },
      scales: {
        x: {
          type: "linear",
          ticks: {
            maxTicksLimit: 6,
            callback: function (value) { return new Date(value).toLocaleTimeString(); }
          },
          grid: { display: false }
        },
        y: {
          min: yMin,
          max: yMax,
          ticks: {
            maxTicksLimit: 5,
            callback: function (value) { return value + unit; }
          }
        }
      },
      plugins: {
        legend: { display: showLegend, position: "bottom", labels: { boxWidth: 12, boxHeight: 2 } }
      }
    };
  }

  function lineChart(canvasId, label, color, options) {
    var canvas = document.getElementById(canvasId);
    if (!canvas) return null;
    return new Chart(canvas, {
      type: "line",
      data: { datasets: [{ label: label, data: [], borderColor: color, backgroundColor: color }] },
      options: options
    });
  }

  function build(sample) {
    var temp = null;
    var tempCanvas = document.getElementById("temp-chart");
    if (tempCanvas) {
      // One dataset per sensor, from the first sample: hwmon/thermal sensors do not appear
      // or disappear while the page is open.
      temp = new Chart(tempCanvas, {
        type: "line",
        data: {
          datasets: sample.temps.map(function (sensor, i) {
            var color = TEMP_COLORS[i % TEMP_COLORS.length];
            return { label: sensor.label, data: [], borderColor: color, backgroundColor: color };
          })
        },
        options: lineOptions(undefined, undefined, "°C", true)
      });
    }

    return {
      cpu: lineChart("cpu-chart", "CPU", CPU_COLOR, lineOptions(0, 100, "%", false)),
      mem: lineChart("mem-chart", "Memory", MEM_COLOR, lineOptions(0, 100, "%", false)),
      temp: temp
    };
  }

  function append(sample) {
    if (!charts) return;

    if (charts.cpu) push(charts.cpu.data.datasets[0].data, sample.taken_at_ms, sample.cpu_total);
    if (charts.mem) push(charts.mem.data.datasets[0].data, sample.taken_at_ms, sample.memory.used_percent);

    if (charts.temp) {
      charts.temp.data.datasets.forEach(function (dataset) {
        var sensor = sample.temps.find(function (s) { return s.label === dataset.label; });
        if (sensor) push(dataset.data, sample.taken_at_ms, sensor.celsius);
      });
    }

    [charts.cpu, charts.mem, charts.temp].forEach(function (chart) {
      if (chart) chart.update("none");
    });
  }

  function init() {
    // No Chart.js (CDN blocked): the panel's numbers still work, so leave them alone.
    if (typeof Chart === "undefined") return;
    var sample = readSample();
    if (!sample) return;

    charts = build(sample);
    append(sample);

    document.body.addEventListener("htmx:afterSwap", function () {
      var next = readSample();
      if (next) append(next);
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
