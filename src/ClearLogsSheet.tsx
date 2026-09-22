import { Icon } from './icons';
import { Sheet } from './Sheet';

export interface ClearLogsSheetProps {
  open: boolean;
  /** True while an action is in flight; confirming is disabled until it settles. */
  busy: boolean;
  onDismiss: () => void;
  onConfirm: () => void;
}

/** Confirmation for the one destructive action: truncating the log file. */
export function ClearLogsSheet({ open, busy, onDismiss, onConfirm }: ClearLogsSheetProps) {
  return (
    <Sheet open={open} onDismiss={onDismiss} labelledBy="clear-logs-title">
      <div className="sheet-head">
        <div className="grow">
          <h3 id="clear-logs-title">清空日志？</h3>
          <p>日志文件会被直接截断，已写入的内容无法恢复。</p>
        </div>
      </div>
      <div className="sheet-foot">
        <button className="btn ghost" data-autofocus onClick={onDismiss}>
          取消
        </button>
        <button className="btn danger push" disabled={busy} onClick={onConfirm}>
          <Icon name="trash" />
          清空日志
        </button>
      </div>
    </Sheet>
  );
}
