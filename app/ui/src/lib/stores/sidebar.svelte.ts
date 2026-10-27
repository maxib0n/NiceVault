import * as api from '../api';
import { isMobile } from '../platform';

class SidebarState {
  collapsed = $state(false);
  get rail(): boolean {
    return this.collapsed && !isMobile;
  }

  adopt(s: Partial<Pick<api.AppSettings, 'sidebar_collapsed'>>) {
    this.collapsed = s.sidebar_collapsed ?? false;
  }

  toggle() {
    if (isMobile) return;
    const next = !this.collapsed;
    this.collapsed = next;
    void api.appSettingsSet({ sidebar_collapsed: next }).catch(() => {
      this.collapsed = !next;
    });
  }
}

export const sidebar = new SidebarState();
