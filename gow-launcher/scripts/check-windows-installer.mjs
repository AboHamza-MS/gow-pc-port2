// Guards the Windows release contract. A clean Windows installation must not need WebView2 beforehand or an
// internet connection just to open the launcher: NSIS carries Microsoft's offline WebView2 installer.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const configPath = resolve('src-tauri', 'tauri.conf.json');
const config = JSON.parse(readFileSync(configPath, 'utf8'));
const windows = config.bundle?.windows;
const webview = windows?.webviewInstallMode;

if (webview?.type !== 'offlineInstaller' || webview.silent !== true) {
  throw new Error(
    `${configPath} must set bundle.windows.webviewInstallMode to { type: "offlineInstaller", silent: true }; ` +
      'the Windows release must run on a clean PC without downloading WebView2.',
  );
}

if (windows?.nsis?.installMode !== 'currentUser') {
  throw new Error(`${configPath} must keep the NSIS installer in currentUser mode.`);
}

console.log('Windows installer contract: offline WebView2 and per-user NSIS installation.');
