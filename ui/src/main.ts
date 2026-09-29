import { mount } from 'svelte';
import './lib/design/tokens.css';
import './lib/design/base.css';
import App from './App.svelte';
import { api } from './lib/ipc';
import { installMotionTokens } from './lib/design/motion';
import { browser } from './lib/stores/browser.svelte';

installMotionTokens();

// No browser context menu or default drag-and-drop on the chrome itself.
addEventListener('contextmenu', (e) => {
  const el = e.target as HTMLElement;
  if (!el.closest('input, textarea, [contenteditable]')) e.preventDefault();
});
addEventListener('dragover', (e) => e.preventDefault());
addEventListener('drop', (e) => e.preventDefault());

const app = mount(App, { target: document.getElementById('app')! });

browser.init().then(() => {
  // Show the window only once the first real frame is painted (no white flash).
  requestAnimationFrame(() => requestAnimationFrame(() => void api.appReady()));
});

export default app;
