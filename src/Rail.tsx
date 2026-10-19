import { getCurrentWindow } from '@tauri-apps/api/window';

import { QuitApp } from './api';
import { Icon } from './icons';
import { VIEW_NAV, VIEW_ORDER, type View } from './navigation';

export interface RailProps {
  view: View;
  onSelectView: (view: View) => void;
  /** Phone shell: the rail is the bottom tab bar, and desktop-only buttons drop out. */
  mobile: boolean;
}

/**
 * The left rail on the desktop, the bottom tab bar on a phone — one component,
 * because a second navigator would drift from this one's order and its a11y.
 */
export function Rail({ view, onSelectView, mobile }: RailProps) {
  return (
    // The drag attribute stays in both shapes: on a phone no drag handler is
    // installed, so it degrades to a plain cursor hint instead of a second
    // markup branch. The phone stylesheet hides the brand and re-flows the rest.
    <aside className="pop rail" data-app-drag>
      <div className="rail-brand" data-app-drag>
        <div className="mark" data-app-drag>
          ET
        </div>
        <div className="wordmark" data-app-drag>
          EasyTier Manager
        </div>
      </div>

      <nav className="rail-nav" aria-label="主导航">
        {VIEW_ORDER.map((entry) => (
          <button
            key={entry}
            className={`rail-item${view === entry ? ' active' : ''}`}
            aria-current={view === entry ? 'page' : undefined}
            onClick={() => onSelectView(entry)}
          >
            <Icon name={VIEW_NAV[entry].icon} />
            {VIEW_NAV[entry].label}
          </button>
        ))}
      </nav>

      <div className="rail-spacer" data-app-drag />

      <div className="rail-foot">
        {/* Android has no tray to hide into — the window *is* the app — so the
            button is not rendered rather than rendered disabled: it would
            promise a background state the platform does not have. */}
        {!mobile && (
          // `close` rather than a bare `hide`: the window's own close button goes
          // through the same path, so both read as one action. The backend
          // intercepts it and hides instead of quitting.
          <button className="rail-item" onClick={() => void getCurrentWindow().close()}>
            <Icon name="minimize" />
            隐藏到托盘
          </button>
        )}
        <button className="rail-item" onClick={() => void QuitApp()}>
          <Icon name="exit" />
          退出
        </button>
      </div>
    </aside>
  );
}
