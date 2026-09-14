import { getCurrentWindow } from '@tauri-apps/api/window';

import type { Status } from './api';
import { Icon } from './icons';

export interface CompactCardProps {
  status: Status;
  busy: string;
  busyLabel: string;
  message: string;
  error: string;
  onInstall: () => void;
  onStart: () => void;
  onStop: () => void;
  onExpand: () => void;
}

/**
 * The compact form factor: an at-a-glance card with a single primary action.
 * Deeper surfaces (logs, settings, update checks) live behind 展开.
 */
export function CompactCard({
  status,
  busy,
  busyLabel,
  message,
  error,
  onInstall,
  onStart,
  onStop,
  onExpand,
}: CompactCardProps) {
  const working = busy !== '';
  const stateLabel = status.running ? '运行中' : status.installed ? '已停止' : '未安装';
  const tone = status.running ? 'success' : status.installed ? 'amber' : 'muted';
  const ringIcon = working ? 'loader' : status.running ? 'check' : status.installed ? 'pause' : 'download';
  const sub = status.installed ? (status.version ? `v${status.version}` : '已安装') : '尚未安装';
  const notice = error || message;

  return (
    <section className="pop compact" data-app-drag>
      <header className="compact-head" data-app-drag>
        <div className="mark" data-app-drag>
          ET
        </div>
        <div className="compact-actions">
          <button className="iconbtn" onClick={onExpand} aria-label="展开" title="展开">
            <Icon name="expand" />
          </button>
          <button
            className="iconbtn"
            onClick={() => void getCurrentWindow().minimize()}
            aria-label="最小化"
            title="最小化"
          >
            <Icon name="minimize" />
          </button>
          <button
            className="iconbtn close"
            onClick={() => void getCurrentWindow().close()}
            aria-label="隐藏到托盘"
            title="隐藏到托盘"
          >
            <Icon name="close" />
          </button>
        </div>
      </header>

      <div className="compact-body">
        <div className={`ring ${tone}${working ? ' busy' : ''}`} data-app-drag>
          <Icon name={ringIcon} />
        </div>
        <h2>{stateLabel}</h2>
        <p className="sub">{sub}</p>
      </div>

      <div className="compact-foot">
        {!status.installed ? (
          <button className="btn big primary" disabled={working} onClick={onInstall}>
            <Icon name="download" />
            {working ? busyLabel : '安装 EasyTier'}
          </button>
        ) : status.running ? (
          <button className="btn big danger" disabled={working} onClick={onStop}>
            <Icon name="stop" />
            {working ? busyLabel : '停止'}
          </button>
        ) : (
          <button className="btn big primary" disabled={working} onClick={onStart}>
            <Icon name="play" />
            {working ? busyLabel : '启动'}
          </button>
        )}
        {notice && (
          <p className={`compact-notice${error ? ' error' : ''}`} role="status" aria-live="polite">
            {notice}
          </p>
        )}
      </div>
    </section>
  );
}
