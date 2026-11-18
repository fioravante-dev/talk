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

  // O quadrado da tecla grava o que se aperta nele, em vez de escrever: a
  // tecla pelo lugar no teclado (e.code), para Alt+D não virar "∂". Apertada
  // com modificadoras, marca as modificadoras também. Um atalho que já está
  // registrado nem chega aqui (o sistema o pega antes): por isso as
  // modificadoras também se marcam à mão.
  var MODS = { ctrl: "ctrlKey", alt: "altKey", shift: "shiftKey", win: "metaKey" };
  document.addEventListener("keydown", function (e) {
    var input = e.target;
    if (!input.matches || !input.matches("[data-keycap]")) return;
    if (e.key === "Tab" || e.key === "Enter") return;
    e.preventDefault();
    if (e.key === "Backspace" || e.key === "Delete") { input.value = ""; return; }
    if (e.key === "Escape") { var d = input.closest("details"); if (d) d.open = false; return; }
    var m = /^(?:Key([A-Z])|Digit(\d)|Numpad(\d)|(F\d{1,2}))$/.exec(e.code);
    if (!m) return; // uma modificadora sozinha, ou uma tecla que não serve
    input.value = m[1] || m[2] || m[3] || m[4];
    if (e.ctrlKey || e.altKey || e.metaKey) {
      Object.keys(MODS).forEach(function (name) {
        var box = input.form.querySelector('input[name="' + name + '"]');
        if (box) box.checked = e[MODS[name]];
      });
    }
  });
})();
