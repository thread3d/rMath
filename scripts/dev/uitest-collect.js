// Phase 2 of scripts/dev/uitest.sh: collect the outcome as JSON.
JSON.stringify({
  errs: window.__errs,
  cells: document.querySelectorAll(".cell").length,
  form: currentForm,
  formUnchanged: currentForm === window.__form0,
  results: [...document.querySelectorAll(".outrow .content")]
             .map(function (e) { return e.textContent.trim(); }).slice(0, 3)
})
