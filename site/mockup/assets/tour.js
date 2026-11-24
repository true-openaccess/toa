// TOA project site. The tour: one step at a time, each played as a few beats.
//
// In a step, an element with data-at="n" gets the class "on" at beat n (and "hot" while n is the
// latest beat), and one with data-off="n" gets "off" at beat n; site.css says what each does.
// Beat 0 is shown at once, beat n after n beats. With reduced motion every step is shown complete.
(function () {
  'use strict';
  var BEAT = 1100;          // ms between beats
  var HOLD = 2800;          // ms the last beat stays before "Play all steps" moves on

  document.addEventListener('DOMContentLoaded', function () {
    var tour = document.querySelector('.tour');
    if (!tour) return;
    var reduce = window.matchMedia && matchMedia('(prefers-reduced-motion: reduce)').matches;
    var tabs = [].slice.call(tour.querySelectorAll('[role="tab"]'));
    var steps = [].slice.call(tour.querySelectorAll('.tstep'));
    var count = tour.querySelector('[data-n]');
    var auto = tour.querySelector('[data-go="auto"]');
    var cur = 0, timers = [], frozen = null;

    function all(step, sel) { return [].slice.call(step.querySelectorAll(sel)); }
    function last(step) {
      return all(step, '[data-at],[data-off]').reduce(function (m, el) {
        return Math.max(m, +(el.getAttribute('data-at') || 0), +(el.getAttribute('data-off') || 0));
      }, 0);
    }
    function beat(step, b, max) {
      all(step, '[data-at]').forEach(function (el) {
        var at = +el.getAttribute('data-at');
        el.classList.toggle('on', at <= b);
        el.classList.toggle('hot', at === b && b > 0 && !reduce);
      });
      all(step, '[data-off]').forEach(function (el) {
        el.classList.toggle('off', +el.getAttribute('data-off') <= b);
      });
      tabs[cur].style.setProperty('--p', max ? Math.max(b, 0) / max : 1);
    }
    function stop() { timers.forEach(clearTimeout); timers = []; }

    function select(i) {
      cur = (i + steps.length) % steps.length;
      tabs.forEach(function (t, k) {
        var on = k === cur;
        t.setAttribute('aria-selected', on ? 'true' : 'false');
        t.tabIndex = on ? 0 : -1;
        if (on) t.classList.add('seen');
      });
      steps.forEach(function (s, k) { s.classList.toggle('cur', k === cur); });
      if (count) count.textContent = String(cur + 1);
    }
    function show(i, focus) {
      stop();
      select(i);
      if (focus) tabs[cur].focus();
      var step = steps[cur], max = last(step);
      if (reduce) { beat(step, max, max); return; }
      if (frozen !== null) { beat(step, Math.min(frozen, max), max); return; }
      beat(step, -1, max);
      void step.offsetWidth;   // start the transitions from the hidden state
      for (var b = 0; b <= max; b++) {
        (function (b) { timers.push(setTimeout(function () { beat(step, b, max); }, b ? b * BEAT : 30)); })(b);
      }
      if (auto && auto.checked) timers.push(setTimeout(function () { show(cur + 1); }, max * BEAT + HOLD));
    }

    tabs.forEach(function (t, k) {
      t.addEventListener('click', function () { show(k); });
      t.addEventListener('keydown', function (e) {
        if (e.key === 'ArrowRight') { e.preventDefault(); show(cur + 1, true); }
        if (e.key === 'ArrowLeft') { e.preventDefault(); show(cur - 1, true); }
      });
    });
    tour.addEventListener('click', function (e) {
      var go = e.target.closest && e.target.closest('button[data-go]');
      if (!go) return;
      var what = go.getAttribute('data-go');
      show(what === 'next' ? cur + 1 : what === 'prev' ? cur - 1 : cur);
    });
    if (auto) auto.addEventListener('change', function () { show(cur); });

    // A link to #ts-N opens step N; so does ?step=N, without scrolling (for screenshots).
    var m = /^#ts-(\d+)$/.exec(location.hash), q = new URLSearchParams(location.search).get('step');
    var n = m ? +m[1] : q ? +q : 0;
    // ?beat=N holds the step at beat N, for pictures of the animation.
    var fb = new URLSearchParams(location.search).get('beat');
    frozen = fb === null ? null : +fb;
    if (n >= 1 && n <= steps.length) {
      select(n - 1);
      if (m) {
        // Now, and again after the browser's own jump to the hash, which found no shown element.
        var go = function () { tour.scrollIntoView({ behavior: 'instant' }); };
        go();
        window.addEventListener('load', function () { setTimeout(go, 0); });
      }
    }
    // Start the step when the tour comes into view, so its animation is seen.
    var first = steps[cur];
    if (!reduce && frozen === null && 'IntersectionObserver' in window) {
      beat(first, -1, last(first));
      var seen = new IntersectionObserver(function (entries) {
        if (entries.some(function (en) { return en.isIntersecting; })) { seen.disconnect(); show(cur); }
      }, { rootMargin: '0px 0px -35% 0px' });
      seen.observe(tour.querySelector('.tour-box'));
    } else {
      show(cur);
    }
  });
})();
