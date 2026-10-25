// Which shell the UI runs in (no imports: api.ts and the stores read it).

/** The Tauri mobile webviews (Android WebView, iOS WKWebView); `?platform=mobile` for screenshots in a browser. */
export function detectMobile(ua: string = navigator.userAgent, search: string = location.search): boolean {
  const forced = new URLSearchParams(search).get('platform');
  if (forced) return forced === 'mobile';
  return /Android|iPhone|iPad|iPod/i.test(ua);
}

/** Android / iOS: no folder picker, no "open with", pickers return URIs. */
export const isMobile: boolean = typeof navigator !== 'undefined' && typeof location !== 'undefined' && detectMobile();

/** iOS (WKWebView; iPadOS reports itself as a Mac with touch): its own document picker (mobile_pick_files). */
export function detectIos(ua: string = navigator.userAgent, touchPoints: number = navigator.maxTouchPoints ?? 0): boolean {
  return /iPhone|iPad|iPod/i.test(ua) || (/Macintosh/.test(ua) && touchPoints > 1);
}

export const isIos: boolean = isMobile && typeof navigator !== 'undefined' && detectIos();
