// The volume's sidebar folded to a slim rail (desktop only; Ctrl/Cmd+B or its toggle). Kept in the
// app settings (`sidebar_collapsed`): installed mode remembers it, portable / mock for the session.
import * as api from '../api';
import { isMobile } from '../platform';

class SidebarState {
  collapsed = $state(false);
  /** phones draw their own flap / rail: never folded there */
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
