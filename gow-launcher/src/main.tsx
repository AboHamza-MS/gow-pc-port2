import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import buttonRedUrl from './assets/theme/button-red.webp';
import panelUrl from './assets/theme/panel.webp';
import './styles.css';

// GOW-Port: the two theme images only dialogs use start downloading and decoding while the launcher boots (see
// PrewarmDialogImages in App.tsx), so the first dialog never opens without its parchment.
for (const url of [panelUrl, buttonRedUrl]) {
  const image = new Image();
  image.src = url;
  void image.decode?.().catch(() => undefined);
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
