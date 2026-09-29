// Phase 1 of scripts/dev/uitest.sh: exercise the notebook controls.
// Only non-destructive controls are clicked, and the typesetting form and the
// Show-steps setting are restored, so running the test does not alter the
// user's notebook.
window.__errs = [];
window.addEventListener("error", function (e) {
  window.__errs.push((e.message || "") + " @ " +
    String((e.error && e.error.stack) || "").split("\n")[1]);
});
window.__form0 = currentForm;
window.__steps0 = localStorage.getItem("rmath-steps");

document.getElementById("inp").value = "1 + 1\nFactor[x^2 - 1]";
document.getElementById("go").click();

function copySourceOfLastIn() {
  var els = document.querySelectorAll(".cell .inrow .content");
  if (!els.length) return "";
  var el = els[els.length - 1];
  try {
    var range = document.createRange();
    range.selectNodeContents(el);
    var sel = window.getSelection();
    sel.removeAllRanges();
    sel.addRange(range);
    var dt = new DataTransfer();
    el.dispatchEvent(new ClipboardEvent("copy",
      {clipboardData: dt, bubbles: true, cancelable: true}));
    sel.removeAllRanges();
    return dt.getData("text/plain");
  } catch (e) { return "err: " + e; }
}

function startStepsProbe() {
  var sb = document.getElementById("stepsbtn");
  if (!sb) return;
  if (sb.textContent.indexOf("Off") >= 0) sb.click();   // turn Steps on
  document.getElementById("inp").value = "D[Sin[x]^2, x]";
  document.getElementById("go").click();
  setTimeout(function () {
    window.__stepsFound = document.querySelectorAll(".steps .step").length;
    if (localStorage.getItem("rmath-steps") !== window.__steps0) sb.click();
    // A typeset In line must copy as its source, not rendered glyphs.
    window.__copyText = copySourceOfLastIn();
    // The session is autosaved (debounced) after each evaluation; assert it.
    fetch('/api/session', {method: 'POST',
      headers: {'Content-Type': 'application/json'}, body: '{}'})
      .then(function (r) { return r.json(); })
      .then(function (s) { window.__sessionInputs = (s && s.inputs) || []; })
      .catch(function () { window.__sessionInputs = []; });
  }, 3000);
}

setTimeout(function () {
  var safe = ["evalInput", "evalAll", "saveNotebook", "zoomIn", "zoomOut", "showHelp"];
  document.querySelectorAll("#ribbon .tb").forEach(function (b) {
    if (safe.indexOf(b.dataset.action) >= 0) b.click();
  });
  var form = document.getElementById("formbtn");
  if (form) form.click();                       // toggle the form
  var helpClose = document.getElementById("helpclose");
  if (helpClose) helpClose.click();
  // Open both floating panels so the style assertions below can see them.
  if (window.showLean) showLean();
  if (window.showAgent) showAgent("");
  setForm(window.__form0);                      // restore the original form

  // A single expression that spans lines must stay one cell: the kernel's own
  // statement splitter decides (previously each line became its own cell, so
  // `N[Gamma[\n21]]` was broken into syntax errors). Poll, because Evaluate All
  // may still be adding cells when this runs.
  document.getElementById("inp").value = "N[Gamma[\n21]]";
  document.getElementById("go").click();
  var tries = 0;
  var poll = setInterval(function () {
    tries += 1;
    var found = "";
    document.querySelectorAll(".cell .inrow .content").forEach(function (c) {
      var t = c.title || "";
      if (t.indexOf("N[Gamma[") >= 0 && t.indexOf("21]]") >= 0) found = t;
    });
    if (found || tries > 16) {
      clearInterval(poll);
      window.__multiLineInput = found;
      startStepsProbe();
    }
  }, 500);
}, 2500);
