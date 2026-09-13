import { Icon, type IconName } from '../icons';
import { useTheme, type ThemeMode } from '../theme';
import type { Status } from '../api';

export interface SettingsProps {
  status: Status;
  configServer: string;
  draftConfigServer: string;
  busy: string;
  onDraftChange: (value: string) => void;
  onSave: () => void;
  onRevert: () => void;
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
  busy,
  onDraftChange,
  onSave,
  onRevert,
}: SettingsProps) {
  const { mode, setMode } = useTheme();
  const working = busy !== '';
  const dirty = draftConfigServer.trim() !== configServer.trim();
  const canSave = dirty && draftConfigServer.trim() !== '' && !working;

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
              <strong>特权助手</strong>
              <span>
                {status.adminReady
                  ? '已授权，后续操作无需密码'
                  : status.adminError || '首次操作时授权一次'}
              </span>
            </div>
            <div className="value">
              <span className={`badge ${status.adminReady ? 'success' : 'amber'}`}>
                <span className="dot" />
                {status.adminReady ? '就绪' : '待授权'}
              </span>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
