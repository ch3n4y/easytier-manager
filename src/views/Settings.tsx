import { Icon, type IconName } from '../icons';
import { useTheme, type ThemeMode } from '../theme';
import type { AppUpdateInfo, Status } from '../api';
import { DEFAULT_GITHUB_PROXY } from '../defaults';

export interface SettingsProps {
  status: Status;
  configServer: string;
  draftConfigServer: string;
  githubProxy: string;
  draftGithubProxy: string;
  appUpdate: AppUpdateInfo | null;
  busy: string;
  onDraftChange: (value: string) => void;
  onSave: () => void;
  onRevert: () => void;
  onGithubProxyChange: (value: string) => void;
  onSaveGithubProxy: () => void;
  onResetGithubProxy: () => void;
  onCheckAppUpdate: () => void;
  onInstallAppUpdate: () => void;
}

const THEME_OPTIONS: Array<{ mode: ThemeMode; icon: IconName; label: string }> = [
  { mode: 'system', icon: 'monitor', label: '跟随系统' },
  { mode: 'light', icon: 'sun', label: '浅色' },
  { mode: 'dark', icon: 'moon', label: '深色' },
];

export function Settings({
  status,
  configServer,
  draftConfigServer,
  githubProxy,
  draftGithubProxy,
  appUpdate,
  busy,
  onDraftChange,
  onSave,
  onRevert,
  onGithubProxyChange,
  onSaveGithubProxy,
  onResetGithubProxy,
  onCheckAppUpdate,
  onInstallAppUpdate,
}: SettingsProps) {
  const { mode, setMode } = useTheme();
  const working = busy !== '';
  const dirty = draftConfigServer.trim() !== configServer.trim();
  const canSave = dirty && draftConfigServer.trim() !== '' && !working;
  const proxyDirty = draftGithubProxy.trim() !== githubProxy.trim();
  const canSaveProxy = proxyDirty && !working;

  // macOS authorizes once through a persistent helper; Windows runs the whole
  // app elevated, so the panel reports that instead of a pending prompt.
  const windows = status.platform === 'windows';
  const privilege = windows
    ? {
        title: '管理员权限',
        badge: status.adminReady ? '已提权' : '未提权',
        detail: status.adminReady ? '已以管理员身份运行' : '请以管理员身份重新运行',
      }
    : {
        title: '特权助手',
        badge: status.adminReady ? '就绪' : '待授权',
        detail: status.adminReady ? '已授权，后续操作无需密码' : '首次操作时授权一次',
      };
  const privilegeDetail = status.adminReady
    ? privilege.detail
    : status.adminError || privilege.detail;

  return (
    <div className="view">
      <section className="panel">
        <div className="panel-head">
          <div className="grow">
            <h3>控制台地址</h3>
          </div>
        </div>
        <div className="panel-body stack">
          <label className="field">
            <input
              aria-label="控制台地址"
              value={draftConfigServer}
              spellCheck={false}
              autoComplete="off"
              placeholder="udp://config.example.com:22020/admin"
              onChange={(event) => onDraftChange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && canSave) onSave();
              }}
            />
            <p className="hint">保存后重启服务生效</p>
          </label>
          <div className="actions">
            {dirty && (
              <button className="btn ghost" disabled={working} onClick={onRevert}>
                还原
              </button>
            )}
            <button className="btn primary push" disabled={!canSave} onClick={onSave}>
              <Icon name="check" />
              保存配置
            </button>
          </div>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">
          <div className="grow">
            <h3>下载加速地址</h3>
          </div>
        </div>
        <div className="panel-body stack">
          <label className="field">
            <input
              aria-label="下载加速地址"
              value={draftGithubProxy}
              spellCheck={false}
              autoComplete="off"
              placeholder={DEFAULT_GITHUB_PROXY}
              onChange={(event) => onGithubProxyChange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && canSaveProxy) onSaveGithubProxy();
              }}
            />
            <p className="hint">
              用于加速从 GitHub 下载 EasyTier 与管理器的安装包和更新；留空或恢复默认即使用
              {DEFAULT_GITHUB_PROXY}
            </p>
          </label>
          <div className="actions">
            {proxyDirty && (
              <button className="btn ghost" disabled={working} onClick={onResetGithubProxy}>
                恢复默认
              </button>
            )}
            <button
              className="btn primary push"
              disabled={!canSaveProxy}
              onClick={onSaveGithubProxy}
            >
              <Icon name="check" />
              保存
            </button>
          </div>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">
          <div className="grow">
            <h3>外观</h3>
          </div>
        </div>
        <div className="panel-body">
          <div className="seg">
            {THEME_OPTIONS.map((option) => (
              <button
                key={option.mode}
                className={mode === option.mode ? 'active' : ''}
                aria-pressed={mode === option.mode}
                onClick={() => setMode(option.mode)}
              >
                <Icon name={option.icon} />
                {option.label}
              </button>
            ))}
          </div>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">
          <div className="grow">
            <h3>权限</h3>
          </div>
        </div>
        <div className="rows">
          <div className="row">
            <div className="label">
              <strong>{privilege.title}</strong>
              <span>{privilegeDetail}</span>
            </div>
            <div className="value">
              <span className={`badge ${status.adminReady ? 'success' : 'amber'}`}>
                <span className="dot" />
                {privilege.badge}
              </span>
            </div>
          </div>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">
          <div className="grow">
            <h3>关于</h3>
          </div>
        </div>
        <div className="rows">
          <div className="row">
            <div className="label">
              <strong>EasyTier Manager</strong>
              <span>{status.appVersion && `v${status.appVersion}`}</span>
            </div>
            <div className="value">
              {appUpdate?.hasUpdate ? (
                <button className="btn primary sm" disabled={working} onClick={onInstallAppUpdate}>
                  <Icon name="download" />
                  更新至 v{appUpdate.latestVersion}
                </button>
              ) : (
                <button className="btn ghost sm" disabled={working} onClick={onCheckAppUpdate}>
                  <Icon name="refresh" />
                  检查更新
                </button>
              )}
            </div>
          </div>
          {appUpdate?.hasUpdate && (appUpdate.notes || appUpdate.date) && (
            <div className="row">
              <div className="label">
                <strong>更新说明</strong>
                {appUpdate.date && <span>发布于 {appUpdate.date}</span>}
                {appUpdate.notes && <span>{appUpdate.notes}</span>}
              </div>
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
