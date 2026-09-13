import { Icon } from '../icons';
import type { Status, UpdateInfo } from '../api';

export interface OverviewProps {
  status: Status;
  configServer: string;
  updateInfo: UpdateInfo | null;
  busy: string;
  logs: string;
  onInstall: () => void;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onCheckUpdate: () => void;
  onUpdateCore: () => void;
  onOpenSettings: () => void;
  onRefreshLogs: () => void;
  onOpenClearLogs: () => void;
}

export function Overview({
  status,
  configServer,
  updateInfo,
  busy,
  logs,
  onInstall,
  onStart,
  onStop,
  onRestart,
  onCheckUpdate,
  onUpdateCore,
  onOpenSettings,
  onRefreshLogs,
  onOpenClearLogs,
}: OverviewProps) {
  const working = busy !== '';
  const stateLabel = status.running ? '运行中' : status.installed ? '已停止' : '未安装';
  const tone = status.running ? 'success' : status.installed ? 'amber' : 'muted';
  const ringIcon = working ? 'loader' : status.running ? 'check' : status.installed ? 'pause' : 'download';

  const sub = status.installed ? (status.version ? `v${status.version}` : '已安装') : '尚未安装';

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
      <section className="panel">
        <div className="hero">
          <div className={`ring ${tone}${working ? ' busy' : ''}`}>
            <Icon name={ringIcon} />
          </div>
          <div className="hero-copy">
            <div className="kicker">EasyTier 服务</div>
            <h1>{stateLabel}</h1>
            <p className="sub">{sub}</p>
            <p className="server" title={configServer}>
              {configServer || '尚未配置控制台地址'}
            </p>
          </div>
        </div>

        <div className="hero-actions">
          {!status.installed ? (
            <button className="btn primary" disabled={working} onClick={onInstall}>
              <Icon name="download" />
              安装 EasyTier
            </button>
          ) : status.running ? (
            <button className="btn danger" disabled={working} onClick={onStop}>
              <Icon name="stop" />
              停止
            </button>
          ) : (
            <button className="btn primary" disabled={working} onClick={onStart}>
              <Icon name="play" />
              启动
            </button>
          )}
          {status.installed && status.running && (
            <button className="btn ghost" disabled={working} onClick={onRestart}>
              <Icon name="refresh" />
              重启
            </button>
          )}
          {status.installed &&
            (updateInfo?.hasUpdate ? (
              <button className="btn ghost" disabled={working} onClick={onUpdateCore}>
                <Icon name="download" />
                更新至 v{updateInfo.latestVersion}
              </button>
            ) : (
              <button className="btn ghost" disabled={working} onClick={onCheckUpdate}>
                <Icon name="arrowUp" />
                检查更新
              </button>
            ))}
          {!configServer && (
            <button
              className="btn ghost"
              disabled={working}
              onClick={onOpenSettings}
            >
              <Icon name="gear" />
              配置
            </button>
          )}
        </div>
      </section>

      <section className="panel fill">
        <div className="panel-head">
          <div className="grow">
            <h3>运行日志</h3>
          </div>
          <div className="stage-actions">
            <button className="btn ghost sm" disabled={working} onClick={onRefreshLogs}>
              <Icon name="refresh" />
              刷新
            </button>
            <button className="btn ghost sm" disabled={!logs} onClick={() => void copyLogs()}>
              <Icon name="copy" />
              复制
            </button>
            <button
              className="btn ghost sm"
              disabled={working || !logs}
              onClick={onOpenClearLogs}
            >
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
