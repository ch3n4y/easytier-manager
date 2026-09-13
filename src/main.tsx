import React from 'react';
import { createRoot } from 'react-dom/client';
import './styles.css';
import App from './App';
import { applyInitialTheme } from './theme';

// Stamp the theme before the first paint so the design tokens are already
// correct when the window's content appears.
applyInitialTheme();

const container = document.getElementById('root');

const root = createRoot(container!);

root.render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
