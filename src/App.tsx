import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import {
  CheckAppUpdate,
  CheckCoreUpdate,
  ClearLogs,
  emptyStatus,
  GetSettings,
  GetStatus,
  InstallAppUpdate,
  InstallLatest,
  ReadConfig,
  ReadLogs,
  RestartService,
  SaveConfig,
  SaveSettings,
  ShowMainWindow,
  StartService,
  StopService,
  UpdateCore,
} from './api';
import type { AppUpdateInfo, DownloadProgress, Status, UpdateInfo } from './api';
import { ClearLogsSheet } from './ClearLogsSheet';
import { DEFAULT_GITHUB_PROXY } from './defaults';
import { Icon } from './icons';
import { VIEW_NAV, type View } from './navigation';
import { Rail } from './Rail';
import { ThemeProvider } from './theme';
import { installWindowDragHandler } from './windowDrag';
import { Overview } from './views/Overview';
import { Settings } from './views/Settings';

type Busy =
  | ''
  | 'install'
  | 'start'
  | 'stop'
  | 'restart'
  | 'save'
  | 'check'
  | 'update'
  | 'appCheck'
  | 'appUpdate'
  | 'logs'
  | 'clear'
  | 'proxy';

const LOG_LIMIT = 200;
const POLL_MS = 5000;
const MB = 1024 * 1024;

const BUSY_LABEL: Record<Exclude<Busy, ''>, string> = {
  install: '安装 EasyTier',
  start: '启动服务',
  stop: '停止服务',
  restart: '重启服务',
  save: '保存配置',
  check: '检查更新',
  update: '更新核心',
  appCheck: '检查管理器更新',
  appUpdate: '更新管理器',
  logs: '加载日志',
  clear: '清空日志',
  proxy: '保存加速地址',
};

/** Actions whose only result is "the service status changed". */
type StatusAction = 'start' | 'stop' | 'restart' | 'update';

/** Busy key, command and confirmation copy for those actions, in one place. */
const STATUS_ACTIONS: Record<StatusAction, { call: () => Promise<Status>; done: string }> = {
  start: { call: StartService, done: '服务已启动' },
  stop: { call: StopService, done: '服务已停止' },
  restart: { call: RestartService, done: '服务已重启' },
  update: { call: UpdateCore, done: 'EasyTier 核心已更新' },
};

/**
 * Release assets are tens of megabytes and are often slow, so the notice has to
 * show real progress — otherwise a long download is indistinguishable from a
 * hang.
 */
function progressText(progress: DownloadProgress | null): string {
  if (!progress) return '';
  const received = (progress.received / MB).toFixed(1);
  if (progress.total && progress.total > 0) {
    const percent = Math.min(100, Math.round((progress.received / progress.total) * 100));
    return `下载中 ${percent}%（${received}/${(progress.total / MB).toFixed(1)} MB）`;
  }
  return `下载中 ${received} MB`;
}

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
  const [appUpdate, setAppUpdate] = useState<AppUpdateInfo | null>(null);
  const [logs, setLogs] = useState('');
  const [busy, setBusy] = useState<Busy>('');
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');
  const [view, setView] = useState<View>('overview');
  const [clearOpen, setClearOpen] = useState(false);
  const [githubProxy, setGithubProxy] = useState('');
  const [draftGithubProxy, setDraftGithubProxy] = useState('');
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  // False until the first status lands. The shell reads every field it renders,
  // so painting it before that would flash empty defaults and then correct them.
  const [ready, setReady] = useState(false);

  // Background polling must not race an in-flight action, and it must not
  // overwrite a config the user is halfway through editing.
  const busyRef = useRef<Busy>('');
  busyRef.current = busy;
  const draftTouched = useRef(false);

  async function run<T>(action: Busy, task: () => Promise<T>, after?: (value: T) => void) {
    setBusy(action);
    setError('');
    setMessage('');
    setProgress(null);
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
    try {
      setLogs(await ReadLogs(LOG_LIMIT));
    } catch {
      // Log tailing is best-effort: a read failure must not blank the status.
    }
  }

  useEffect(() => {
    void readState()
      .catch(() => {
        // First paint can lose the race against the helper install; the poller
        // below picks the state up on its next tick.
      })
      // Whether or not that read worked, the shell is what shows the failure —
      // spinning forever would be the worse answer.
      .finally(() => setReady(true));
    const id = window.setInterval(() => {
      // Editing the config must not freeze the poller: readState only skips the
      // draft field while the user is typing, and still refreshes the status.
      if (busyRef.current === '') void readState().catch(() => undefined);
    }, POLL_MS);
    return () => window.clearInterval(id);
  }, []);

  // The backend starts the window hidden so its first frame is the finished
  // shell rather than an empty one; this is the other half of that bargain.
  // Two frames past mount is the earliest point at which the shell is painted,
  // and the backend unveils anyway if this never runs.
  useEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => {
        void ShowMainWindow().catch(() => undefined);
      });
    });
    return () => window.cancelAnimationFrame(frame);
  }, []);

  // A long download would otherwise look like a hang, so the backend streams
  // bytes as it fetches the release asset.
  useEffect(() => {
    const pending = listen<DownloadProgress>('download-progress', (event) =>
      setProgress(event.payload),
    );
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, []);

  // App-level settings are not part of the polled status.
  useEffect(() => {
    void (async () => {
      try {
        const settings = await GetSettings();
        setGithubProxy(settings.githubProxy);
        setDraftGithubProxy(settings.githubProxy);
      } catch {
        // Best-effort: the backend's default accelerator applies regardless.
      }
    })();
  }, []);

  // The window is frameless, so dragging and the double-click zoom are ours.
  useEffect(() => installWindowDragHandler(), []);

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

  /** The four lifecycle actions share one shape: run, then report the new status. */
  function statusAction(key: StatusAction) {
    return run(key, STATUS_ACTIONS[key].call, (nextStatus) => {
      setStatus(nextStatus);
      setMessage(STATUS_ACTIONS[key].done);
    });
  }

  const startService = () => statusAction('start');
  const stopService = () => statusAction('stop');
  const restartService = () => statusAction('restart');
  const updateCore = () => statusAction('update');

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

  async function saveGithubProxy() {
    await run(
      'proxy',
      () => SaveSettings({ githubProxy: draftGithubProxy.trim() }),
      (next) => {
        setGithubProxy(next.githubProxy);
        setDraftGithubProxy(next.githubProxy);
        setMessage('下载加速地址已保存');
      },
    );
  }

  async function checkUpdate() {
    await run('check', CheckCoreUpdate, (info) => {
      setUpdateInfo(info);
      setMessage(info.hasUpdate ? `发现新版本 v${info.latestVersion}` : 'EasyTier 已是最新版本');
    });
  }

  async function checkAppUpdate() {
    await run('appCheck', CheckAppUpdate, (info) => {
      setAppUpdate(info);
      setMessage(info.hasUpdate ? `管理器有新版本 v${info.latestVersion}` : '管理器已是最新版本');
    });
  }

  async function installAppUpdate() {
    await run('appUpdate', InstallAppUpdate, () => {
      setMessage('管理器已更新');
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

  const downloading = busy === 'install' || busy === 'update' || busy === 'appUpdate';
  const busyText = busy
    ? downloading && progress
      ? progressText(progress)
      : BUSY_LABEL[busy]
    : '';

  const title = VIEW_NAV[view].label;

  // The window is already on screen at this point — it is revealed as soon as
  // the shell paints — so this is the first thing the user sees: chrome and a
  // spinner, and nothing that reads as data before the status arrives.
  if (!ready) {
    return (
      <div className="shell">
        <div className="boot" data-app-drag>
          <Icon name="loader" />
        </div>
      </div>
    );
  }

  return (
    <>
      <div className="shell">
        <Rail view={view} onSelectView={setView} />

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
              githubProxy={githubProxy}
              draftGithubProxy={draftGithubProxy}
              appUpdate={appUpdate}
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
              onGithubProxyChange={setDraftGithubProxy}
              onSaveGithubProxy={() => void saveGithubProxy()}
              onResetGithubProxy={() => setDraftGithubProxy(DEFAULT_GITHUB_PROXY)}
              onCheckAppUpdate={() => void checkAppUpdate()}
              onInstallAppUpdate={() => void installAppUpdate()}
            />
          )}
        </div>

        {(busy || message || error) && (
          <div className={`stage-notice${error ? ' error' : ''}`} role="status" aria-live="polite">
            {error ||
              (busy ? (downloading && progress ? busyText : `${busyText}…`) : message)}
          </div>
        )}
        </main>
      </div>

      <ClearLogsSheet
        open={clearOpen}
        busy={busy !== ''}
        onDismiss={() => setClearOpen(false)}
        onConfirm={() => void clearLogs()}
      />
    </>
  );
}

function App() {
  return (
    <ThemeProvider>
      <Shell />
    </ThemeProvider>
  );
}

export default App;
