// Stroke icon set (currentColor, 24×24) — the same visual language as the
// reference design: 2px round strokes, slightly narrower than a full grid, so
// icons read at 16–17px inside buttons and rail rows.

import type { ReactNode } from 'react';

export type IconName =
  | 'house'
  | 'terminal'
  | 'gear'
  | 'play'
  | 'stop'
  | 'refresh'
  | 'download'
  | 'arrowUp'
  | 'check'
  | 'pause'
  | 'alert'
  | 'info'
  | 'loader'
  | 'shield'
  | 'trash'
  | 'copy'
  | 'minimize'
  | 'close'
  | 'exit'
  | 'sun'
  | 'moon'
  | 'monitor';

const PATHS: Record<IconName, ReactNode> = {
  house: (
    <>
      <path d="M3.5 10.8 12 3.8l8.5 7" />
      <path d="M6 9.2v9.3A1.5 1.5 0 0 0 7.5 20h9a1.5 1.5 0 0 0 1.5-1.5V9.2" />
    </>
  ),
  terminal: (
    <>
      <rect x="3.2" y="4.5" width="17.6" height="15" rx="2.4" />
      <polyline points="8 10 11 12.5 8 15" />
      <line x1="13" y1="15" x2="16.6" y2="15" />
    </>
  ),
  gear: (
    <>
      <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
      <circle cx="12" cy="12" r="3" />
    </>
  ),
  play: <path d="M8 5.5v13l10-6.5-10-6.5Z" />,
  stop: <rect x="6.8" y="6.8" width="10.4" height="10.4" rx="2.2" fill="currentColor" stroke="none" />,
  pause: (
    <>
      <line x1="9.5" y1="6" x2="9.5" y2="18" />
      <line x1="14.5" y1="6" x2="14.5" y2="18" />
    </>
  ),
  refresh: (
    <>
      <path d="M20 11.5a8 8 0 1 0-1.9 6.4" />
      <polyline points="20 4.5 20 11.5 13 11.5" />
    </>
  ),
  download: (
    <>
      <path d="M12 3.5v11" />
      <polyline points="7.5 10 12 14.5 16.5 10" />
      <path d="M5 19.5h14" />
    </>
  ),
  arrowUp: (
    <>
      <line x1="12" y1="19" x2="12" y2="5.5" />
      <polyline points="6 11 12 5 18 11" />
    </>
  ),
  check: <polyline points="4 12.5 9.5 18 20 6.5" />,
  alert: (
    <>
      <path d="M12 4 2.6 20h18.8L12 4Z" />
      <line x1="12" y1="10" x2="12" y2="14" />
      <circle cx="12" cy="17.4" r="0.5" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <line x1="12" y1="11" x2="12" y2="16.5" />
      <circle cx="12" cy="7.6" r="0.5" />
    </>
  ),
  loader: <path d="M12 3.5a8.5 8.5 0 1 0 8.5 8.5" />,
  shield: (
    <>
      <path d="M12 3 5 5.7v5.3c0 4.3 3 7.6 7 9.5 4-1.9 7-5.2 7-9.5V5.7L12 3Z" />
      <polyline points="9 12 11 14 15 9.6" />
    </>
  ),
  trash: (
    <>
      <path d="M5 7h14" />
      <path d="M9.5 7V5.2A1.2 1.2 0 0 1 10.7 4h2.6a1.2 1.2 0 0 1 1.2 1.2V7" />
      <path d="M6.5 7l.8 12.1A1.3 1.3 0 0 0 8.6 20.4h6.8a1.3 1.3 0 0 0 1.3-1.3L17.5 7" />
    </>
  ),
  copy: (
    <>
      <path d="M5 15.5V6.5A1.5 1.5 0 0 1 6.5 5h9" />
      <rect x="8" y="8" width="11" height="11" rx="1.5" />
    </>
  ),
  minimize: <line x1="6" y1="12" x2="18" y2="12" />,
  exit: (
    <>
      <path d="M12 3.5v8" />
      <path d="M7.4 6.3a7 7 0 1 0 9.2 0" />
    </>
  ),
  close: (
    <>
      <line x1="6.5" y1="6.5" x2="17.5" y2="17.5" />
      <line x1="17.5" y1="6.5" x2="6.5" y2="17.5" />
    </>
  ),
  sun: (
    <>
      <circle cx="12" cy="12" r="4.2" />
      <path d="M12 2.6v2.1M12 19.3v2.1M2.6 12h2.1M19.3 12h2.1M5.4 5.4l1.5 1.5M17.1 17.1l1.5 1.5M18.6 5.4l-1.5 1.5M6.9 17.1l-1.5 1.5" />
    </>
  ),
  moon: <path d="M20 13.6A8.6 8.6 0 0 1 10.4 4a8.6 8.6 0 1 0 9.6 9.6Z" />,
  monitor: (
    <>
      <rect x="3.2" y="4.8" width="17.6" height="11.4" rx="2.2" />
      <line x1="12" y1="16.2" x2="12" y2="19.4" />
      <line x1="8.4" y1="19.4" x2="15.6" y2="19.4" />
    </>
  ),
};

export function Icon({ name, className }: { name: IconName; className?: string }) {
  return (
    <svg
      className={['icon', `icon-${name}`, className].filter(Boolean).join(' ')}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {PATHS[name]}
    </svg>
  );
}
