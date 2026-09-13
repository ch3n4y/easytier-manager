import { useEffect, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  CheckCoreUpdate,
  ClearLogs,
  emptyStatus,
  GetStatus,
  InstallLatest,
  ReadConfig,
  ReadLogs,
  RestartService,
  SaveConfig,
  StartService,
  StopService,
  UpdateCore,
} from './api';
import type { Status, UpdateInfo } from './api';
import { CompactCard } from './CompactCard';
import { Icon } from './icons';
import { Sheet } from './Sheet';
import { ThemeProvider } from './theme';
import { installWindowDragHandler } from './windowDrag';
import { WindowModeProvider, useWindowMode } from './windowMode';
import { Overview } from './views/Overview';
import { Settings } from './views/Settings';

type View = 'overview' | 'settings';
type Busy =
  | ''
  | 'install'
  | 'start'
  | 'stop'
  | 'restart'
  | 'save'
  | 'check'
  | 'update'
  | 'logs'
  | 'clear';

const LOG_LIMIT = 200;
const POLL_MS = 5000;

const BUSY_LABEL: Record<string, string> = {
  install: '安装 EasyTier',
  start: '启动服务',
  stop: '停止服务',
  restart: '重启服务',
  save: '保存配置',
  check: '检查更新',
  update: '更新核心',
  logs: '加载日志',
  clear: '清空日志',
};

const VIEW_META: Record<View, string> = {
  overview: '概览',
  settings: '设置',
};

function errorText(err: unknown): string {
  if (typeof err === 'string') return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

function Shell() {
  const [status, setStatus] = useState<Status>(emptyStatus);
  const [configServer, setConfigServer] = useState('');
  const [draftConfigServer, setDraftConfigServer] = useState('');
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [logs, setLogs] = useState('');
  const [busy, setBusy] = useState<Busy>('');
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');
  const [view, setView] = useState<View>('overview');
  const [clearOpen, setClearOpen] = useState(false);

  const { mode, setMode } = useWindowMode();
  const modeRef = useRef(mode);
  modeRef.current = mode;

  // Background polling must not race an in-flight action, and it must not
  // overwrite a config the user is halfway through editing.
  const busyRef = useRef<Busy>('');
  busyRef.current = busy;
  const draftTouched = useRef(false);

  async function run<T>(action: Busy, task: () => Promise<T>, after?: (value: T) => void) {
    setBusy(action);
    setError('');
    setMessage('');
    try {
      const value = await task();
      after?.(value);
      return value;
    } catch (err) {
      setError(errorText(err));
      return undefined;
    } finally {
      setBusy('');
    }
  }

  /** Silent reads for the poller; the loud path drives the busy footer. */
  async function readState() {
    const [nextStatus, nextConfig] = await Promise.all([GetStatus(), ReadConfig()]);
    setStatus(nextStatus);
    setConfigServer(nextConfig.configServer || '');
    if (!draftTouched.current) setDraftConfigServer(nextConfig.configServer || '');
    // The log tail only exists in the expanded workbench; skip the read while
    // the compact card is up.
    if (modeRef.current === 'expanded') {
      try {
        setLogs(await ReadLogs(LOG_LIMIT));
      } catch {
        // Log tailing is best-effort: a read failure must not blank the status.
      }
    }
  }

  useEffect(() => {
    void readState().catch(() => {
      // First paint can lose the race against the helper install; the poller
      // below picks the state up on its next tick.
    });
    const id = window.setInterval(() => {
      // Editing the config must not freeze the poller: readState only skips the
      // draft field while the user is typing, and still refreshes the status.
      if (busyRef.current === '') void readState().catch(() => undefined);
    }, POLL_MS);
    return () => window.clearInterval(id);
  }, []);

  // Expanding reveals the log panel; fill it immediately instead of waiting
  // for the next poll tick.
  useEffect(() => {
    if (mode === 'expanded') void readState().catch(() => undefined);
  }, [mode]);

  // The compact card is fixed-size, so double-click must not maximize it.
  useEffect(
    () => installWindowDragHandler({ canToggleMaximize: () => modeRef.current === 'expanded' }),
    [],
  );

  useEffect(() => {
    if (!message) return;
    const id = window.setTimeout(() => setMessage(''), 3200);
    return () => window.clearTimeout(id);
  }, [message]);

  async function installLatest() {
    await run('install', InstallLatest, (nextStatus) => {
      setStatus(nextStatus);
      setMessage('EasyTier 已安装');
      void readState().catch(() => undefined);
    });
  }

  async function startService() {
    await run('start', StartService, (nextStatus) => {
      setStatus(nextStatus);
      setMessage('服务已启动');
    });
  }

  async function stopService() {
    await run('stop', StopService, (nextStatus) => {
      setStatus(nextStatus);
      setMessage('服务已停止');
    });
  }

  async function restartService() {
    await run('restart', RestartService, (nextStatus) => {
      setStatus(nextStatus);
      setMessage('服务已重启');
    });
  }

  async function saveConfig() {
    const server = draftConfigServer.trim();
    await run(
      'save',
      () => SaveConfig({ mode: 'web', configServer: server, defaultConf: '' }),
      (nextConfig) => {
        const saved = nextConfig.configServer || server;
        draftTouched.current = false;
        setConfigServer(saved);
        setDraftConfigServer(saved);
        setMessage('配置已保存，重启服务后生效');
      },
    );
  }

  async function checkUpdate() {
    await run('check', CheckCoreUpdate, (info) => {
      setUpdateInfo(info);
      setMessage(info.hasUpdate ? `发现新版本 v${info.latestVersion}` : 'EasyTier 已是最新版本');
    });
  }

  async function updateCore() {
    await run('update', UpdateCore, (nextStatus) => {
      setStatus(nextStatus);
      setMessage('EasyTier 核心已更新');
    });
  }

  async function refreshLogs() {
    await run('logs', () => ReadLogs(LOG_LIMIT), setLogs);
  }

  async function clearLogs() {
    await run('clear', ClearLogs, (nextStatus) => {
      setStatus(nextStatus);
      setLogs('');
      setMessage('日志已清空');
    });
    // Close the sheet whether or not the clear succeeded, so a failure surfaces
    // in the stage notice instead of leaving the modal stuck open.
    setClearOpen(false);
  }

  if (mode === 'compact') {
    return (
      <CompactCard
        status={status}
        busy={busy}
        busyLabel={busy ? BUSY_LABEL[busy] : ''}
        message={message}
        error={error}
        onInstall={() => void installLatest()}
        onStart={() => void startService()}
        onStop={() => void stopService()}
        onExpand={() => setMode('expanded')}
      />
    );
  }

  const title = VIEW_META[view];

  return (
    <>
      <div className="shell">
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
          <button
            className={`rail-item${view === 'overview' ? ' active' : ''}`}
            aria-current={view === 'overview' ? 'page' : undefined}
            onClick={() => setView('overview')}
          >
            <Icon name="house" />
            概览
          </button>
          <button
            className={`rail-item${view === 'settings' ? ' active' : ''}`}
            aria-current={view === 'settings' ? 'page' : undefined}
            onClick={() => setView('settings')}
          >
            <Icon name="gear" />
            设置
          </button>
        </nav>

        <div className="rail-spacer" data-app-drag />

        <button className="rail-item rail-collapse" onClick={() => setMode('compact')}>
          <Icon name="collapse" />
          收起
        </button>

        <div className="rail-foot">
          <button className="rail-item" onClick={() => void getCurrentWindow().minimize()}>
            <Icon name="minimize" />
            最小化
          </button>
          <button className="rail-item" onClick={() => void getCurrentWindow().close()}>
            <Icon name="close" />
            关闭窗口
          </button>
        </div>
        </aside>

        <main className="pop stage">
        <header className="stage-head" data-app-drag>
          <div className="titles" data-app-drag>
            <h2>{title}</h2>
          </div>
        </header>

        <div className={`scroll${view === 'overview' ? ' flush' : ''}`}>
          {view === 'overview' && (
            <Overview
              status={status}
              configServer={configServer}
              updateInfo={updateInfo}
              busy={busy}
              logs={logs}
              onInstall={() => void installLatest()}
              onStart={() => void startService()}
              onStop={() => void stopService()}
              onRestart={() => void restartService()}
              onCheckUpdate={() => void checkUpdate()}
              onUpdateCore={() => void updateCore()}
              onOpenSettings={() => setView('settings')}
              onRefreshLogs={() => void refreshLogs()}
              onOpenClearLogs={() => setClearOpen(true)}
            />
          )}
          {view === 'settings' && (
            <Settings
              status={status}
              configServer={configServer}
              draftConfigServer={draftConfigServer}
              busy={busy}
              onDraftChange={(value) => {
                draftTouched.current = true;
                setDraftConfigServer(value);
              }}
              onSave={() => void saveConfig()}
              onRevert={() => {
                draftTouched.current = false;
                setDraftConfigServer(configServer);
              }}
            />
          )}
        </div>

        {(busy || message || error) && (
          <div className={`stage-notice${error ? ' error' : ''}`} role="status" aria-live="polite">
            {error || (busy ? `${BUSY_LABEL[busy]}…` : message)}
          </div>
        )}
        </main>
      </div>

      <Sheet open={clearOpen} onDismiss={() => setClearOpen(false)} labelledBy="clear-logs-title">
        <div className="sheet-head">
          <div className="grow">
            <h3 id="clear-logs-title">清空日志？</h3>
            <p>日志文件会被直接截断，已写入的内容无法恢复。</p>
          </div>
        </div>
        <div className="sheet-foot">
          <button className="btn ghost" data-autofocus onClick={() => setClearOpen(false)}>
            取消
          </button>
          <button className="btn danger push" disabled={busy !== ''} onClick={() => void clearLogs()}>
            <Icon name="trash" />
            清空日志
          </button>
        </div>
      </Sheet>
    </>
  );
}

function App() {
  return (
    <ThemeProvider>
      <WindowModeProvider>
        <Shell />
      </WindowModeProvider>
    </ThemeProvider>
  );
}

export default App;
