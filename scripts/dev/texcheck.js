// Phase 2 of scripts/dev/texcheck.sh: every typeset span in the page is parsed
// by KaTeX with throwOnError, so a TeX printer bug (like a bare ^{\circ}) is
// reported instead of silently falling back to raw text.
JSON.stringify((function () {
  const bad = [];
  document.querySelectorAll("span.tex").forEach(function (el) {
    [["std", el.dataset.texStd], ["trad", el.dataset.texTrad]].forEach(function (pair) {
      if (!pair[1]) return;
      try {
        katex.renderToString(pair[1], {throwOnError: true});
      } catch (e) {
        bad.push(pair[0] + " :: " + pair[1].slice(0, 80) + "  <<" + e.message.slice(0, 70));
      }
    });
  });
  return {
    spans: document.querySelectorAll("span.tex").length,
    badCount: bad.length,
    bad: bad.slice(0, 20),
  };
})())
