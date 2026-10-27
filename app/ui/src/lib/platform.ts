export function detectMobile(ua: string = navigator.userAgent, search: string = location.search): boolean {
  const forced = new URLSearchParams(search).get('platform');
  if (forced) return forced === 'mobile';
  return /Android|iPhone|iPad|iPod/i.test(ua);
}

export const isMobile: boolean = typeof navigator !== 'undefined' && typeof location !== 'undefined' && detectMobile();

export function detectIos(ua: string = navigator.userAgent, touchPoints: number = navigator.maxTouchPoints ?? 0): boolean {
  return /iPhone|iPad|iPod/i.test(ua) || (/Macintosh/.test(ua) && touchPoints > 1);
}

export const isIos: boolean = isMobile && typeof navigator !== 'undefined' && detectIos();
