// Phase 1 of scripts/dev/uitest.sh: exercise the notebook controls.
// Only non-destructive controls are clicked, and the typesetting form is
// restored, so running the test does not alter the user's notebook.
window.__errs = [];
window.addEventListener("error", function (e) {
  window.__errs.push((e.message || "") + " @ " +
    String((e.error && e.error.stack) || "").split("\n")[1]);
});
window.__form0 = currentForm;

document.getElementById("inp").value = "1 + 1\nFactor[x^2 - 1]";
document.getElementById("go").click();

setTimeout(function () {
  var safe = ["evalInput", "evalAll", "saveNotebook", "zoomIn", "zoomOut", "showHelp"];
  document.querySelectorAll("#ribbon .tb").forEach(function (b) {
    if (safe.indexOf(b.dataset.action) >= 0) b.click();
  });
  var form = document.getElementById("formbtn");
  if (form) form.click();                       // toggle the form
  var helpClose = document.getElementById("helpclose");
  if (helpClose) helpClose.click();
  setForm(window.__form0);                      // restore the original form
}, 2500);
