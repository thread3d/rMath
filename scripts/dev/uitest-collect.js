// Phase 2 of scripts/dev/uitest.sh: collect the outcome as JSON.
//
// `styles` exists because a bulk CSS edit once deleted a whole panel's rules and
// nothing noticed until the user did: these assertions pin the computed styles
// that a missing or mangled rule would change.
JSON.stringify((function () {
  var lean = document.getElementById("lean");
  var agent = document.getElementById("agent");
  var log = document.getElementById("agentlog");
  function cs(el, prop) { return el ? getComputedStyle(el)[prop] : null; }
  return {
    errs: window.__errs,
    cells: document.querySelectorAll(".cell").length,
    form: currentForm,
    formUnchanged: currentForm === window.__form0,
    steps: window.__stepsFound || 0,
    stepsRestored: localStorage.getItem("rmath-steps") === window.__steps0,
    multiLineInput: window.__multiLineInput || "",
    agentModels: document.querySelectorAll("#agentmodel option").length,
    copyText: window.__copyText || "",
    sessionInputs: window.__sessionInputs || [],
    results: [...document.querySelectorAll(".outrow .content")]
               .map(function (e) { return e.textContent.trim(); }).slice(0, 3),
    styles: {
      leanFixed: cs(lean, "position") === "fixed",
      leanWidth: lean ? Math.round(lean.getBoundingClientRect().width) : 0,
      leanDisplay: cs(lean, "display"),
      leanHeaderPadding: cs(lean && lean.querySelector("header"), "paddingTop"),
      stmtPadding: cs(document.getElementById("leanstmt"), "paddingTop"),
      diagMaxHeight: cs(document.getElementById("leandiag"), "maxHeight"),
      agentFixed: cs(agent, "position") === "fixed",
      agentWidth: agent ? Math.round(agent.getBoundingClientRect().width) : 0,
      logBackground: cs(log, "backgroundColor"),
      logOverflow: cs(log, "overflowY"),
      inLabelColor: cs(document.querySelector(".inlbl"), "color"),
      texFontSize: cs(document.querySelector(".tex"), "fontSize"),
      ribbonButtons: document.querySelectorAll("#ribbon .tb").length
    }
  };
})())
