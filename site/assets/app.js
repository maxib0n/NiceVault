const REPO = 'maxib0n/NiceVault';

(() => {
  'use strict';
  const REPO_URL = `https://github.com/${REPO}`;
  const RELEASES = `${REPO_URL}/releases`;
  const LATEST = `${RELEASES}/latest/download/`;

  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];

  $$('[data-repo]').forEach((a) => (a.href = REPO_URL));
  $$('[data-releases]').forEach((a) => (a.href = RELEASES));
  $$('[data-asset]').forEach((a) => (a.href = LATEST + a.dataset.asset));

  function detect() {
    const uad = navigator.userAgentData;
    const ua = navigator.userAgent || '';
    const plat = ((uad && uad.platform) || navigator.platform || '').toLowerCase();
    if (/android/i.test(ua)) return 'android';
    if (/iphone|ipad|ipod/i.test(ua) || (plat === 'macintel' && navigator.maxTouchPoints > 1)) return 'ios';
    if (/win/.test(plat) || /windows/i.test(ua)) return 'windows';
    if (/mac/.test(plat) || /macintosh/i.test(ua)) return 'macos';
    if (/linux|x11|cros/.test(plat) || /linux|x11/i.test(ua)) return 'linux';
    return 'unknown';
  }
  const OS = {
    linux: { label: 'Download for Linux', asset: 'NiceVault-linux-x86_64.AppImage', note: 'x86_64 · AppImage · usable' },
    windows: { label: 'Download for Windows', asset: 'NiceVault-windows-x86_64.zip', note: 'x86_64 · zip · usable' },
    macos: { label: 'Download for macOS', asset: 'NiceVault-macos-universal.zip', note: 'universal · zip · builds, not fully tested' },
  };
  const os = detect();
  const main = $('#dl-main');
  const note = $('#dl-note');
  const pick = OS[os];
  if (pick) {
    main.textContent = pick.label;
    main.href = LATEST + pick.asset;
    note.textContent = pick.note;
    const row = $('#dl-table tr[data-os="' + os + '"]');
    if (row) row.classList.add('here');
  } else if (os === 'ios' || os === 'android') {
    main.textContent = 'See downloads';
    note.textContent = 'There is no phone build to install yet. iOS and Android are in development.';
  } else {
    main.textContent = 'See downloads';
  }

  const viewer = $('#viewer');
  const tabs = $$('[role="tab"]', viewer);
  function select(tab, focus) {
    tabs.forEach((t) => {
      const on = t === tab;
      t.setAttribute('aria-selected', on);
      t.tabIndex = on ? 0 : -1;
      $('#' + t.getAttribute('aria-controls')).hidden = !on;
    });
    if (focus) tab.focus();
    document.dispatchEvent(new CustomEvent('nv:tab'));
  }
  tabs.forEach((t, i) => {
    t.addEventListener('click', () => select(t));
    t.addEventListener('keydown', (e) => {
      const k = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
      if (k) { e.preventDefault(); select(tabs[(i + k + tabs.length) % tabs.length], true); }
      if (e.key === 'Home') { e.preventDefault(); select(tabs[0], true); }
      if (e.key === 'End') { e.preventDefault(); select(tabs[tabs.length - 1], true); }
    });
  });

  const noise = $('#noise');
  if (noise) {
    const h = '0123456789abcdef';
    let s = '';
    for (let i = 0; i < 900; i++) s += h[(Math.random() * 16) | 0];
    noise.textContent = s;
  }

  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;

  const vids = $$('.clips video');
  if ('IntersectionObserver' in window && !reduce) {
    const vo = new IntersectionObserver((es) => es.forEach((e) => {
      if (e.isIntersecting) e.target.play().catch(() => {});
      else e.target.pause();
    }), { threshold: 0.35 });
    vids.forEach((v) => vo.observe(v));
  } else {
    vids.forEach((v) => { v.controls = true; });
  }
})();
