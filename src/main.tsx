import React from 'react';
import { createRoot } from 'react-dom/client';
import './styles.css';
import App from './App';
import { applyInitialTheme } from './theme';

// Stamp the theme before the first paint so the design tokens are already
// correct when the window's content appears.
applyInitialTheme();

// Stamp the host platform before the first paint too: the stylesheet drops the
// transparent window gutter on Windows, and waiting for the first status poll
// would flash it in. Read from the user agent rather than the backend because
// this has to happen synchronously.
document.documentElement.dataset.platform = /Windows/i.test(navigator.userAgent)
  ? 'windows'
  : 'macos';

const container = document.getElementById('root');

const root = createRoot(container!);

root.render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
