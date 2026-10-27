export function fade(node: HTMLElement, from: number, to: number, opts: KeyframeAnimationOptions): Animation {
  const prev = { opacity: node.style.opacity, willChange: node.style.willChange };
  node.style.opacity = 'var(--nv-o)';
  node.style.willChange = 'opacity';
  const a = node.animate([{ '--nv-o': String(from) }, { '--nv-o': String(to) }] as Keyframe[], opts);
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
