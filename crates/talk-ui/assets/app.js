// O que o serviço empurra para a janela (via evaluate_script), no instante do
// evento: fase, nível do microfone, ditada nova. Nada aqui consulta nada.
(function () {
  function app() { return document.getElementById("app"); }

  window.talkEvent = function (e) {
    var root = app();
    if (!root) return;
    if (e.event === "phase") {
      root.dataset.phase = e.phase;
      if (e.phase !== "recording") paint(0);
      setClock(e.seconds || 0);
    } else if (e.event === "level") {
      root.dataset.phase = "recording";
      paint(e.level);
      setClock(e.seconds);
    } else if (e.event === "dictation") {
      // Recarrega a tela atual: o rodapé e, no histórico, a ditada nova.
      var url = location.pathname + location.search;
      htmx.ajax("GET", url, { target: "#app", select: "#app", swap: "outerHTML" });
    }
  };

  function setClock(s) {
    var el = document.querySelector("[data-clock]");
    if (el) el.textContent = Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0");
  }

  // RMS da fala fica entre ~0,01 e ~0,3: escala logarítmica, 16 segmentos.
  function paint(level) {
    var meter = document.querySelector("[data-meter]");
    if (!meter) return;
    var db = level > 0 ? 20 * Math.log10(level) : -60;
    var lit = Math.max(0, Math.min(16, Math.round((db + 50) / 45 * 16)));
    meter.querySelectorAll("i").forEach(function (seg, i) {
      seg.classList.toggle("lit", i < lit);
      seg.classList.toggle("hot", i >= 14 && i < lit);
    });
  }
})();
