import { Icon } from '../icons';

export interface LogsProps {
  logs: string;
  busy: string;
  onRefresh: () => void;
  onClear: () => void;
}

export function Logs({ logs, busy, onRefresh, onClear }: LogsProps) {
  const working = busy !== '';

  async function copyLogs() {
    if (!logs) return;
    try {
      await navigator.clipboard.writeText(logs);
    } catch {
      // Clipboard can be unavailable (no focus, denied permission); the text is
      // selectable in place, so this stays a silent no-op.
    }
  }

  return (
    <div className="view">
      <section className="panel fill">
        <div className="panel-head actions-only">
          <div className="stage-actions">
            <button className="btn ghost sm" disabled={working} onClick={onRefresh}>
              <Icon name="refresh" />
              刷新
            </button>
            <button className="btn ghost sm" disabled={!logs} onClick={() => void copyLogs()}>
              <Icon name="copy" />
              复制
            </button>
            <button className="btn ghost sm" disabled={working || !logs} onClick={onClear}>
              <Icon name="trash" />
              清空
            </button>
          </div>
        </div>
        <pre className={logs ? 'logs' : 'logs logs-empty'}>{logs || '暂无日志。'}</pre>
      </section>
    </div>
  );
}
