// ARGUS dashboard — keyboard navigation for the cohort view.
//
// The cohort view is a CodeRabbit-style "Change Stack" of layers. To move
// through the stack without a mouse, press:
//   j / J  -> next layer
//   k / K  -> previous layer
//   g      -> first layer
//   G      -> last  layer
//
// Every <article.layer> in templates.rs carries tabindex="0" so it can
// receive focus, and a data-layer-id so the order is stable.

(function () {
  'use strict';

  function collectLayers() {
    return Array.prototype.slice.call(
      document.querySelectorAll('article.layer')
    );
  }

  document.addEventListener('keydown', function (e) {
    // Don't hijack typing inside form fields.
    var t = e.target;
    if (
      t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)
    ) {
      return;
    }

    var articles = collectLayers();
    if (articles.length === 0) return;

    var focused = document.activeElement;
    var idx = articles.indexOf(focused);
    var next = idx;

    if (e.key === 'j' || e.key === 'J') {
      if (idx === -1) next = 0;
      else next = Math.min(articles.length - 1, idx + 1);
    } else if (e.key === 'k' || e.key === 'K') {
      if (idx === -1) next = 0;
      else next = Math.max(0, idx - 1);
    } else if (e.key === 'g') {
      next = 0;
    } else if (e.key === 'G') {
      next = articles.length - 1;
    } else {
      return;
    }

    if (next !== idx && next >= 0 && next < articles.length) {
      e.preventDefault();
      var target = articles[next];
      target.focus();
      // Bring the focused layer into view if it's below the fold.
      if (typeof target.scrollIntoView === 'function') {
        target.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
      }
    }
  });
})();
