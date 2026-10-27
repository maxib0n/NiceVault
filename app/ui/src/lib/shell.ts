const editable = (t: EventTarget | null) =>
  typeof Element !== 'undefined' && t instanceof Element && !!t.closest('input:not([type=checkbox]):not([type=radio]):not([type=range]), textarea, [contenteditable=""], [contenteditable="true"]');

export interface ShellOptions {
  app: boolean;
  devMenu: boolean;
}

export function installShellGuards(win: Window, o: ShellOptions): () => void {
  const menu = (e: MouseEvent) => {
    if (o.devMenu || editable(e.target)) return;
    e.preventDefault();
  };
  const keys = (e: KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    const reload = e.key === 'F5' || (mod && (e.key === 'r' || e.key === 'R'));
    const nav = e.key === 'BrowserBack' || e.key === 'BrowserForward' || (e.altKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight') && !editable(e.target));
    if (o.app && (reload || nav)) e.preventDefault();
  };
  const buttons = (e: MouseEvent) => {
    if (o.app && (e.button === 3 || e.button === 4)) e.preventDefault();
  };
  win.addEventListener('contextmenu', menu);
  win.addEventListener('keydown', keys, true);
  win.addEventListener('mouseup', buttons, true);
  win.addEventListener('mousedown', buttons, true);
  return () => {
    win.removeEventListener('contextmenu', menu);
    win.removeEventListener('keydown', keys, true);
    win.removeEventListener('mouseup', buttons, true);
    win.removeEventListener('mousedown', buttons, true);
  };
}
