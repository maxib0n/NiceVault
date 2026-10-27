export function originOf(sender, reported) {
  let o = 'null';
  if (sender && 'origin' in sender && typeof sender.origin === 'string') o = sender.origin;
  else {
    try {
      o = new URL(sender?.url).origin;
    } catch {
      o = 'null';
    }
  }
  if (reported === 'null') return 'null';
  return o || 'null';
}

const KINDS = {
  content: ['frame_fields', 'open_menu'],
  menu: ['menu_view', 'menu_search', 'menu_fill', 'menu_launch', 'menu_unlock', 'menu_resize', 'menu_close', 'show_app'],
  page: ['popup_view', 'popup_search', 'popup_fill', 'popup_copy', 'popup_unlock', 'pair_start', 'show_app', 'get_state', 'unpair_local', 'forget_memory'],
};

export function senderRole(sender, extBase, extId) {
  if (!sender || sender.id !== extId) return null;
  const url = typeof sender.url === 'string' ? sender.url : '';
  if (url.startsWith(extBase)) {
    const path = url.slice(extBase.length).split(/[?#]/)[0];
    return path === 'menu.html' ? 'menu' : 'page';
  }
  return sender.tab ? 'content' : null;
}

export function allowedFrom(kind, sender, extBase, extId) {
  const role = senderRole(sender, extBase, extId);
  return Boolean(role && KINDS[role].includes(kind));
}
