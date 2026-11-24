// TOA project site. Theme, language menu and the contents of the standard's page.
(function () {
  var root = document.documentElement;
  // Scripts are on: the tour hides the steps not being shown.
  root.classList.add('js');
  function stored() { try { return localStorage.getItem('toa-theme'); } catch (e) { return null; } }
  function store(v) { try { localStorage.setItem('toa-theme', v); } catch (e) {} }
  var forced = new URLSearchParams(location.search).get('theme');
  var theme = forced || stored();
  if (theme === 'light' || theme === 'dark') root.setAttribute('data-theme', theme);

  function current() {
    var t = root.getAttribute('data-theme');
    if (t) return t;
    return window.matchMedia && matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }

  document.addEventListener('DOMContentLoaded', function () {
    var btn = document.getElementById('theme-btn');
    if (btn) {
      var label = function () {
        // The labels are the page's own, in its language.
        btn.setAttribute('aria-label', current() === 'dark' ? btn.dataset.light : btn.dataset.dark);
      };
      label();
      btn.addEventListener('click', function () {
        var next = current() === 'dark' ? 'light' : 'dark';
        root.setAttribute('data-theme', next);
        store(next);
        label();
      });
    }

    // The language menu closes on Escape and on a click outside it.
    var menu = document.querySelector('details.lang');
    if (menu) {
      document.addEventListener('click', function (e) { if (!menu.contains(e.target)) menu.open = false; });
      document.addEventListener('keydown', function (e) { if (e.key === 'Escape') menu.open = false; });
    }

    // Contents of the standard: mark the section being read.
    var links = Array.prototype.slice.call(document.querySelectorAll('.toc a[href^="#"]'));
    if (links.length && 'IntersectionObserver' in window) {
      var byId = {};
      links.forEach(function (a) { byId[decodeURIComponent(a.getAttribute('href').slice(1))] = a; });
      var heads = Array.prototype.slice.call(document.querySelectorAll('.prose h2[id], .prose h3[id]'));
      var visible = {};
      var obs = new IntersectionObserver(function (entries) {
        entries.forEach(function (en) { visible[en.target.id] = en.isIntersecting; });
        var first = heads.find(function (h) { return visible[h.id]; });
        if (!first) return;
        links.forEach(function (a) { a.classList.remove('on'); });
        var a = byId[first.id];
        if (a) {
          a.classList.add('on');
          var box = a.closest('.toc');
          if (box && (a.offsetTop < box.scrollTop || a.offsetTop > box.scrollTop + box.clientHeight - 40)) {
            box.scrollTop = a.offsetTop - box.clientHeight / 3;
          }
        }
      }, { rootMargin: '-70px 0px -65% 0px' });
      heads.forEach(function (h) { obs.observe(h); });
    }
  });
})();
