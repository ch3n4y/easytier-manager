import { useEffect, useRef } from 'react';
import { Icon } from '../icons';
import type { Status } from '../api';

export interface OverviewProps {
  status: Status;
  configServer: string;
  busy: string;
  logs: string;
  onInstall: () => void;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onOpenSettings: () => void;
  onRefreshLogs: () => void;
  onOpenClearLogs: () => void;
}

export function Overview({
  status,
  configServer,
  busy,
  logs,
  onInstall,
  onStart,
  onStop,
  onRestart,
  onOpenSettings,
  onRefreshLogs,
  onOpenClearLogs,
}: OverviewProps) {
  const working = busy !== '';
  const stateLabel = status.running ? '运行中' : status.installed ? '已停止' : '未安装';
  const tone = status.running ? 'success' : status.installed ? 'amber' : 'muted';
  const ringIcon = working ? 'loader' : status.running ? 'check' : status.installed ? 'pause' : 'download';

  const sub = status.installed ? (status.version ? `v${status.version}` : '已安装') : '尚未安装';
  // The log pane follows the tail, but only while the reader is already at the
  // bottom: pulling the view back while someone is scrolling through history
  // would fight them every poll. `stuck` records that intent, and a fresh
  // log payload (a manual refresh replaces the whole string) resets it so the
  // result of an explicit action is always visible.
  const logRef = useRef<HTMLPreElement>(null);
  const stuck = useRef(true);

  useEffect(() => {
    const pane = logRef.current;
    if (pane && stuck.current) pane.scrollTop = pane.scrollHeight;
  }, [logs]);

  function trackLogScroll() {
    const pane = logRef.current;
    if (!pane) return;
    // A few pixels of slack: fractional line heights otherwise leave the pane
    // permanently "not quite at the bottom" and the follow never re-arms.
    stuck.current = pane.scrollHeight - pane.scrollTop - pane.clientHeight < 24;
  }

  function revealLatest() {
    const pane = logRef.current;
    if (!pane) return;
    stuck.current = true;
    pane.scrollTop = pane.scrollHeight;
  }


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
            <button
              className="btn ghost sm"
              disabled={working}
              onClick={() => {
                // An explicit refresh is a request for the newest lines, so it
                // re-arms the follow even if the reader had scrolled up.
                revealLatest();
                onRefreshLogs();
              }}
            >
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
        <pre
          ref={logRef}
          className={logs ? 'logs' : 'logs logs-empty'}
          onScroll={trackLogScroll}
        >
          {logs || '暂无日志。'}
        </pre>
      </section>
    </div>
  );
}
