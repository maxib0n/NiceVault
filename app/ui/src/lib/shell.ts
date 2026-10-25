// The webview is an app, not a browser: no browser context menu (Back / Forward / Stop /
// Reload) where the app has none of its own, no reload, no history navigation of the page.
//   - context menu: blocked everywhere except text fields (native Cut / Copy / Paste there);
//     kept in a Tauri dev build (Inspect Element).
//   - reload (F5, Ctrl/Cmd+R, Ctrl+Shift+R) and page back/forward (Alt+←/→, the mouse side
//     buttons, BrowserBack/Forward keys): blocked inside Tauri builds; the app's own folder
//     history (FileBrowserView) still gets those events. The plain-browser mock keeps reload.

const editable = (t: EventTarget | null) =>
  typeof Element !== 'undefined' && t instanceof Element && !!t.closest('input:not([type=checkbox]):not([type=radio]):not([type=range]), textarea, [contenteditable=""], [contenteditable="true"]');

export interface ShellOptions {
  /** Tauri: also block reload and page navigation */
  app: boolean;
  /** keep the native context menu (Tauri dev: Inspect Element) */
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
  // bubble phase on window: the app's own handlers (custom menus, folder history) run first
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
