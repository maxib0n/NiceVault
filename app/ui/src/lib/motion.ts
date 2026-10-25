// Fades from script (Web Animations) that never animate `opacity` itself: WebKitGTK runs an
// opacity animation on the compositor and, when it ends, shows one stale frame from its middle
// (a flash). The node reads its opacity from the registered --nv-o (styles/motion.css) while the
// animation runs, and only --nv-o is animated. Same curve, same timing.

/** `node.animate` of its opacity from `from` to `to`, through --nv-o. */
export function fade(node: HTMLElement, from: number, to: number, opts: KeyframeAnimationOptions): Animation {
  const prev = { opacity: node.style.opacity, willChange: node.style.willChange };
  node.style.opacity = 'var(--nv-o)';
  node.style.willChange = 'opacity';
  const a = node.animate([{ '--nv-o': String(from) }, { '--nv-o': String(to) }] as Keyframe[], opts);
  // a fill keeps the end value (the node is on its way out): left as it is
  const restore = () => {
    if (opts.fill === 'forwards' || opts.fill === 'both') return;
    node.style.opacity = prev.opacity;
    node.style.willChange = prev.willChange;
  };
  a.addEventListener('finish', restore);
  a.addEventListener('cancel', () => {
    node.style.opacity = prev.opacity;
    node.style.willChange = prev.willChange;
  });
  return a;
}
