// The glyph decode from the NiceVault app, on the page. Same engine (vendor/ascii-reveal,
// MIT, the one the app bundles) and the same preset as the app's lib/reveal.ts: text and
// pictures resolve out of a grid of machine-readable glyphs, once, when they come into view.
// The real content stays in the DOM the whole time. With prefers-reduced-motion nothing
// plays and nothing is held back.
(() => {
  'use strict';
  const NV = window.NVReveal;
  const reduce = matchMedia('(prefers-reduced-motion: reduce)');
  if (!NV || reduce.matches || !('IntersectionObserver' in window)) { document.documentElement.classList.remove('dec-on'); return; }

  // the machine-readable alphabet of the app's document MRZ, condensing through a light ramp
  const MRZ = {
    ramp: ' .:-=+<#',
    scramble: 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789<',
    mode: 'ramp',
    color: 'accent',
    cells: [18, 12, 9, 7, 5],
    tick: 36,
    accentGlyphs: true,
  };

  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const fontsReady = () => (document.fonts && document.fonts.ready ? Promise.race([document.fonts.ready, new Promise((r) => setTimeout(r, 600))]) : Promise.resolve());

  // buttons and chips fade in on the wave instead of being drawn as glyphs
  $$('.btn, .chip').forEach((el) => el.setAttribute('data-ascii', 'fade'));

  const textTargets = [
    ['.hero-copy', 0.8],
    ['.sec-head', 1],
    ['.facts li', 1],
    ['.steps li', 1],
    ['.limits', 1.3],
    ['.tablewrap', 1.3],
    ['.status dl > div', 1.2],
    ['.todo', 1.2],
    ['.phones-copy', 1],
    ['.clips figcaption', 1.2],
    ['.layout figcaption', 1.2],
    ['.notes', 1.4],
    ['.foot-in', 1.4],
  ];

  function decodeText(el, speed) {
    const c = NV.create(el, { preset: MRZ, speed, origin: 'top-left', lazy: false, media: '[data-ascii="media"]' });
    // held back at once (before any paint), then decoded
    void c.hide({ instant: true });
    el.classList.add('dec-go');
    void c.reveal().then(() => c.destroy());
  }

  function decodePicture(img, host, speed) {
    const go = () => {
      const c = NV.create(host, { preset: MRZ, speed, origin: 'top-left', lazy: false, media: 'img' });
      void c.hide({ instant: true, only: [img] });
      img.classList.add('dec-go');
      void c.reveal({ only: [img] }).then(() => c.destroy());
    };
    const ready = () => (img.decode ? img.decode().then(go, go) : go());
    if (img.complete && img.naturalWidth) ready();
    else {
      img.addEventListener('load', ready, { once: true });
      img.addEventListener('error', () => img.classList.add('dec-go'), { once: true });
    }
  }

  fontsReady().then(() => {
    const io = new IntersectionObserver(
      (entries) => entries.forEach((e) => {
        if (!e.isIntersecting) return;
        io.unobserve(e.target);
        const d = e.target.__dec;
        if (d.pic) decodePicture(e.target.querySelector('img'), e.target, d.speed);
        else decodeText(e.target, d.speed);
      }),
      { threshold: 0.12, rootMargin: '0px 0px -6% 0px' },
    );

    textTargets.forEach(([sel, speed]) => $$(sel).forEach((el) => {
      el.__dec = { speed };
      el.setAttribute('data-dec', '');
      io.observe(el);
    }));

    // pictures: the first one in each place a visitor can see; tabs decode the first time they are opened
    $$('.ph').forEach((el) => { el.__dec = { pic: true, speed: 1.15 }; el.setAttribute('data-decp', ''); el.querySelector('img').setAttribute('data-decimg', ''); io.observe(el); });

    const frame = document.querySelector('.frame');
    const firstImg = frame && frame.querySelector('.panel:not([hidden]) img');
    const seen = new Set();
    const decodeActive = () => {
      const img = frame.querySelector('.panel:not([hidden]) img');
      if (!img || seen.has(img)) return;
      seen.add(img);
      decodePicture(img, frame, 1.1);
    };
    if (frame) {
      frame.querySelectorAll('.panel img').forEach((i) => i.setAttribute('data-decimg', ''));
      const fio = new IntersectionObserver((es) => {
        if (es[0].isIntersecting) { fio.disconnect(); decodeActive(); }
      }, { threshold: 0.3 });
      fio.observe(frame);
      let opened = false;
      new IntersectionObserver((es) => { if (es[0].isIntersecting) opened = true; }, { threshold: 0.3 }).observe(frame);
      document.addEventListener('nv:tab', () => { if (opened) decodeActive(); else if (firstImg) { /* first view still pending */ } });
    }

    // failsafe: anything still held back after a while is shown as it is
    setTimeout(() => {
      // only what is on screen right now; the rest waits for its turn
      $$('[data-dec]:not(.dec-go), [data-decimg]:not(.dec-go)').forEach((el) => {
        const r = el.getBoundingClientRect();
        if (r.top < innerHeight && r.bottom > 0) el.classList.add('dec-go');
      });
    }, 6000);
  });
})();
