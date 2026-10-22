// Progressive enhancement only: every page works without this file.
(function () {
  "use strict";

  // Price history: crosshair snaps to the nearest run, tooltip shows value and time.
  function initChart(wrap) {
    var svg = wrap.querySelector("svg.chart");
    var tip = wrap.querySelector(".chart-tip");
    if (!svg || !tip || svg.dataset.ready) return;
    svg.dataset.ready = "1";
    var points;
    try { points = JSON.parse(svg.getAttribute("data-points")); } catch (e) { return; }
    if (!points.length) return;
    var cross = svg.querySelector(".crosshair");
    var index = points.length - 1;

    function show(i) {
      index = Math.max(0, Math.min(points.length - 1, i));
      var p = points[index];
      cross.setAttribute("x1", p[0]);
      cross.setAttribute("x2", p[0]);
      cross.setAttribute("visibility", "visible");
      tip.querySelector("strong").textContent = p[3];
      tip.querySelector("span").textContent = p[2];
      var box = svg.getBoundingClientRect();
      var scale = box.width / svg.viewBox.baseVal.width;
      var left = p[0] * scale + svg.offsetLeft - wrap.scrollLeft;
      tip.style.left = Math.max(60, Math.min(wrap.clientWidth - 60, left)) + "px";
      tip.hidden = false;
    }
    function hide() {
      cross.setAttribute("visibility", "hidden");
      tip.hidden = true;
    }
    function nearest(clientX) {
      var box = svg.getBoundingClientRect();
      var x = (clientX - box.left) * (svg.viewBox.baseVal.width / box.width);
      var best = 0;
      for (var i = 1; i < points.length; i++) {
        if (Math.abs(points[i][0] - x) < Math.abs(points[best][0] - x)) best = i;
      }
      return best;
    }
    svg.addEventListener("pointermove", function (e) { show(nearest(e.clientX)); });
    svg.addEventListener("pointerleave", hide);
    svg.addEventListener("focus", function () { show(index); });
    svg.addEventListener("blur", hide);
    svg.addEventListener("keydown", function (e) {
      if (e.key === "ArrowLeft") { show(index - 1); e.preventDefault(); }
      if (e.key === "ArrowRight") { show(index + 1); e.preventDefault(); }
      if (e.key === "Home") { show(0); e.preventDefault(); }
      if (e.key === "End") { show(points.length - 1); e.preventDefault(); }
    });
  }

  // Heatmap cells: one floating tooltip, on hover and keyboard focus.
  var heatTip;
  function heatTooltip(cell) {
    if (!heatTip) {
      heatTip = document.createElement("div");
      heatTip.className = "heat-tip";
      heatTip.setAttribute("role", "status");
      heatTip.appendChild(document.createElement("strong"));
      heatTip.appendChild(document.createElement("span"));
      document.body.appendChild(heatTip);
    }
    var parts = cell.getAttribute("data-tip").split(": ");
    heatTip.firstChild.textContent = parts[1] || "";
    heatTip.lastChild.textContent = parts[0];
    var box = cell.getBoundingClientRect();
    heatTip.style.left = box.left + box.width / 2 + "px";
    heatTip.style.top = box.top + "px";
    heatTip.hidden = false;
  }
  function hideHeat() { if (heatTip) heatTip.hidden = true; }
  ["pointerover", "focusin"].forEach(function (type) {
    document.addEventListener(type, function (e) {
      var cell = e.target.closest && e.target.closest("td[data-tip]");
      if (cell) heatTooltip(cell);
    });
  });
  ["pointerout", "focusout"].forEach(function (type) {
    document.addEventListener(type, function (e) {
      if (e.target.closest && e.target.closest("td[data-tip]")) hideHeat();
    });
  });

  // Form: show the stay fields only for round trips; adapt location inputs to their kind.
  var PLACEHOLDERS = {
    airports: "FCO oppure REG, SUF, CTA",
    city: "Milano",
    country: "JP oppure Japan",
    radius: "REG 150 oppure 41.9, 12.5, 150",
    anywhere: "Le mete più economiche",
    same: "",
    "": ""
  };
  function syncMode(form) {
    var checked = form.querySelector("input[name=mode]:checked");
    var round = !checked || checked.value !== "one_way";
    form.querySelectorAll("[data-round-trip]").forEach(function (el) { el.hidden = !round; });
  }
  function syncKind(select) {
    var input = select.parentElement.querySelector("input");
    if (!input) return;
    var kind = select.value;
    input.placeholder = PLACEHOLDERS[kind] || "";
    input.disabled = kind === "anywhere" || kind === "same" || kind === "";
  }
  function initForm(form) {
    if (form.dataset.ready) return;
    form.dataset.ready = "1";
    syncMode(form);
    form.querySelectorAll("select[data-kind]").forEach(syncKind);
    form.addEventListener("change", function (e) {
      if (e.target.name === "mode") syncMode(form);
      if (e.target.matches("select[data-kind]")) syncKind(e.target);
    });
  }

  // Destructive actions ask first.
  document.addEventListener("submit", function (e) {
    var message = e.target.getAttribute("data-confirm");
    if (message && !window.confirm(message)) e.preventDefault();
  });

  function init(root) {
    root.querySelectorAll("[data-chart]").forEach(initChart);
    root.querySelectorAll("#search-form").forEach(initForm);
  }
  document.addEventListener("DOMContentLoaded", function () { init(document); });
  document.addEventListener("htmx:load", function (e) { init(e.target); });
})();
