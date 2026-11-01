// The only module that talks to the Rust side. In a plain browser (`npm run dev`, tests, screenshots) a small
// in-memory mock stands in, so the interface can be developed and checked without the Tauri shell.

import type {
  Bootstrap, DestinationValidation, GameFinished, GameStatus, GamepadCaptured, InstallOutcome,
  InstallProgress, IsoValidation, OptimizerSnapshot, Settings, TexturePackInfo, TexturePackOutcome, TexturePackProgress,
  WizardInfo,
} from './types';

export interface EventMap {
  'settings-changed': Settings;
  'game-running': boolean;
  'game-finished': GameFinished;
  'gamepad-captured': GamepadCaptured;
  'gamepad-failed': { reason: string };
  'iso-searching': boolean;
  'iso-found': string;
  'install-progress': InstallProgress;
  'install-done': InstallOutcome;
  'texture-pack-progress': TexturePackProgress;
  'texture-pack-done': TexturePackOutcome;
  /** The window was asked to close while a texture pack is installing; it stayed open. */
  'close-blocked': undefined;
  optimizer: OptimizerSnapshot;
}

export interface PickOptions {
  title: string;
  directory?: boolean;
  filters?: { name: string; extensions: string[] }[];
  defaultPath?: string;
}

export interface Api {
  bootstrap(locale: string): Promise<Bootstrap>;
  patchSettings(patch: Partial<Settings>): Promise<Settings>;
  resetGameOptions(): Promise<Settings>;
  gameStatus(): Promise<GameStatus>;
  detectPaths(force: boolean): Promise<Settings>;
  detectedIso(): Promise<string>;
  memoryCardDirectory(elf: string): Promise<string>;
  startGame(): Promise<void>; // rejects with a LaunchError
  stopGame(): Promise<void>;
  isGameRunning(): Promise<boolean>;
  openLog(): Promise<void>;
  openInstallLog(): Promise<void>;
  openOptimizerLog(): Promise<void>;
  openDirectory(path: string): Promise<void>;
  openTexturesFolder(): Promise<boolean>;
  texturePackInfo(): Promise<TexturePackInfo>;
  installTexturePack(): Promise<void>;
  createShortcut(desktop: boolean): Promise<boolean>;
  raHasCredential(user: string): Promise<boolean>;
  raSaveCredential(user: string, password: string): Promise<boolean>;
  raClearCredential(): Promise<boolean>;
  gamepadBegin(): Promise<boolean>;
  gamepadCancel(): Promise<void>;
  wizardInfo(): Promise<WizardInfo>;
  validateIso(path: string): Promise<IsoValidation>;
  validateDestination(path: string): Promise<DestinationValidation>;
  findIso(): Promise<void>;
  installStart(args: { iso: string; destination: string; installTools: boolean; vu1MemoryLimitGb: number }): Promise<void>;
  installCancel(): Promise<void>;
  installComplete(args: { destination: string; elf: string; executable: string }): Promise<Settings>;
  optimizerSnapshot(): Promise<OptimizerSnapshot>;
  optimizeNow(): Promise<void>;
  // Window and dialogs.
  on<K extends keyof EventMap>(event: K, handler: (payload: EventMap[K]) => void): Promise<() => void>;
  pick(options: PickOptions): Promise<string | null>;
  onFileDrop(handler: (paths: string[], hovering: boolean) => void): Promise<() => void>;
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  /** Closes the window even while a texture pack is installing (after the player confirmed). */
  forceClose(): Promise<void>;
}

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function createTauriApi(): Promise<Api> {
  const { invoke } = await import('@tauri-apps/api/core');
  const { listen } = await import('@tauri-apps/api/event');
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  const { open } = await import('@tauri-apps/plugin-dialog');
  const call = <T,>(command: string, args?: Record<string, unknown>) => invoke<T>(command, args);
  return {
    bootstrap: (locale) => call('bootstrap', { locale }),
    patchSettings: (patch) => call('patch_settings', { patch }),
    resetGameOptions: () => call('reset_game_options'),
    gameStatus: () => call('game_status'),
    detectPaths: (force) => call('detect_paths', { force }),
    detectedIso: () => call('detected_iso'),
    memoryCardDirectory: (elf) => call('memory_card_directory', { elf }),
    startGame: () => call('start_game'),
    stopGame: () => call('stop_game'),
    isGameRunning: () => call('is_game_running'),
    openLog: () => call('open_log'),
    openInstallLog: () => call('open_install_log'),
    openOptimizerLog: () => call('open_optimizer_log'),
    openDirectory: (path) => call('open_directory', { path }),
    openTexturesFolder: () => call('open_textures_folder'),
    texturePackInfo: () => call('texture_pack_info'),
    installTexturePack: () => call('texture_pack_install'),
    createShortcut: (desktop) => call('create_shortcut', { desktop }),
    raHasCredential: (user) => call('ra_has_credential', { user }),
    raSaveCredential: (user, password) => call('ra_save_credential', { user, password }),
    raClearCredential: () => call('ra_clear_credential'),
    gamepadBegin: () => call('gamepad_begin'),
    gamepadCancel: () => call('gamepad_cancel'),
    wizardInfo: () => call('wizard_info'),
    validateIso: (path) => call('validate_iso', { path }),
    validateDestination: (path) => call('validate_destination', { path }),
    findIso: () => call('find_iso'),
    installStart: (arguments_) => call('install_start', { arguments: arguments_ }),
    installCancel: () => call('install_cancel'),
    installComplete: (arguments_) => call('install_complete', { arguments: arguments_ }),
    optimizerSnapshot: () => call('optimizer_snapshot'),
    optimizeNow: () => call('optimize_now'),
    on: async (event, handler) => listen(event, (e) => handler(e.payload as never)),
    pick: async ({ title, directory, filters, defaultPath }) => {
      const chosen = await open({ title, directory: directory ?? false, multiple: false, filters, defaultPath });
      return typeof chosen === 'string' ? chosen : null;
    },
    onFileDrop: async (handler) =>
      getCurrentWebview().onDragDropEvent((event) => {
        const { type } = event.payload;
        if (type === 'enter' || type === 'over') handler([], true);
        else if (type === 'drop') handler(event.payload.paths, false);
        else handler([], false);
      }),
    minimize: () => getCurrentWindow().minimize(),
    toggleMaximize: () => getCurrentWindow().toggleMaximize(),
    close: () => getCurrentWindow().close(),
    forceClose: () => call('force_close'),
  };
}

// ------------------------------------------------------------------------------------------------ browser mock

export const defaultSettings: Settings = {
  language: '', gpu: 'auto', displayMode: 'fullscreen', widthPx: 1280, heightPx: 800, aspectMode: '', widescreen: true, stretch: false,
  filter: 'bilinear', sharpness: 0.8, fxaa: true, progressive480p: true, internalResolution: '2', fpsLimit: 60,
  vsync: false, showFps: false, elf: '', iso: '', executable: '', skipVideos: true, fastBoot: false, customTextures: false, asyncTextures: true,
  settingsVersion: 0, dmc5Controls: false, padButtons: '', cancelAnimations: true, escQuits: false,
  minimizeOnPlay: true, secondsPlayed: 0, lastSession: 0, keys: '', cheats: '', cheatsShortcut: '',
  retroAchievementsEnabled: false, retroAchievementsUser: '',
  retroAchievementsHardcore: false, retroAchievementsNotifications: true, optimizationDestination: '',
  optimizationEnabled: true, optimizeDuringGame: true, optimizationCompiled: 0,
};

type Handler = (payload: never) => void;

export type MockApi = Api & { emit: <K extends keyof EventMap>(event: K, payload: EventMap[K]) => void };

export function createMockApi(options: { ready?: boolean } = {}): MockApi {
  const ready = options.ready ?? true;
  const listeners = new Map<string, Set<Handler>>();
  const emit = <K extends keyof EventMap>(event: K, payload: EventMap[K]) =>
    listeners.get(event)?.forEach((handler) => handler(payload as never));
  let settings: Settings = {
    ...defaultSettings,
    settingsVersion: 9,
    language: 'en',
    aspectMode: '16:9',
    skipVideos: false,
    elf: ready ? 'C:\\GodOfWar-PC\\game\\SCUS_973.99' : '',
    executable: ready ? 'C:\\GodOfWar-PC\\active\\gow-2005.exe' : '',
    secondsPlayed: ready ? 5 * 3600 + 20 * 60 : 0,
    optimizationDestination: ready ? 'C:\\GodOfWar-PC' : '',
    optimizationCompiled: 120,
  };
  const display = { widthPx: 1920, heightPx: 1080, hz: 144 };
  let running = false;
  const status = (): GameStatus => ({
    game: { exists: ready, valid: ready, modified: false, crc: 'D6385328' },
    data: { ok: ready, iso: '', extracted: ready, chosenIsoMissing: false },
    executable: { exists: ready, executable: ready },
    ready,
  });
  const snapshot = (): OptimizerSnapshot => ({
    configured: ready, compiling: false, captured: 150, compiled: settings.optimizationCompiled, completionRatio: 0.8,
    buildProgress: -1, status: 'idle',
  });
  const merge = (patch: Partial<Settings>) => (settings = { ...settings, ...patch });
  const later = <T,>(value: T, ms = 0) => new Promise<T>((resolve) => setTimeout(() => resolve(value), ms));

  return {
    emit,
    bootstrap: async () => ({
      settings, version: '0.1.0', platform: 'windows', display,
      graphicsCards: [
        { id: 'ahorro', kind: 'integrated', displayName: '', manufacturer: '', driver: '', isDefault: false },
        { id: 'rendimiento', kind: 'dedicated', displayName: '', manufacturer: '', driver: '', isDefault: false },
      ],
      dataDirectory: 'C:\\Users\\player\\AppData\\Roaming\\GOW_PC\\Launcher',
      arguments: { play: false, wizard: false },
    }),
    patchSettings: async (patch) => merge(patch),
    resetGameOptions: async () => merge({ gpu: 'auto', displayMode: 'fullscreen', filter: 'bilinear', sharpness: 0.8, fxaa: true, internalResolution: '2', fpsLimit: 60, vsync: false, showFps: false, cheats: '', cheatsShortcut: '' }),
    gameStatus: async () => status(),
    detectPaths: async () => settings,
    detectedIso: async () => '',
    memoryCardDirectory: async (elf) => (elf ? elf.replace(/[\\/][^\\/]*$/, '') + '\\mc0' : ''),
    startGame: async () => {
      running = true;
      emit('game-running', true);
      setTimeout(() => {
        running = false;
        emit('game-running', false);
        emit('game-finished', { code: 0, failed: false, seconds: 125, summary: '' });
      }, 2500);
    },
    stopGame: async () => undefined,
    isGameRunning: async () => running,
    openLog: async () => undefined,
    openInstallLog: async () => undefined,
    openOptimizerLog: async () => undefined,
    openDirectory: async () => undefined,
    openTexturesFolder: async () => true,
    texturePackInfo: async () => ({
      meta: { size: 3_990_828_210, etag: 'mock', uploaded: '2026-10-29T02:58:32.356Z' },
      status: { installed: false, files: 0, bytes: 0, etag: '', uploaded: '' },
    }),
    installTexturePack: async () => {
      // Short fake run so the page can be developed in a browser.
      setTimeout(
        () => emit('texture-pack-progress', { stage: 'downloading', done: 2_000_000_000, total: 3_990_828_210, overall: 0.4 }),
        200,
      );
      setTimeout(() => emit('texture-pack-progress', { stage: 'extracting', done: 120, total: 420, overall: 0.9 }), 900);
      setTimeout(
        () =>
          emit('texture-pack-done', {
            ok: true,
            status: { installed: true, files: 420, bytes: 5_100_000_000, etag: 'mock', uploaded: '2026-10-29T02:58:32.356Z' },
            error: null,
            code: null,
          }),
        1500,
      );
    },
    createShortcut: async () => true,
    raHasCredential: async (user) => user === 'player',
    raSaveCredential: async () => true,
    raClearCredential: async () => true,
    gamepadBegin: async () => {
      setTimeout(() => emit('gamepad-captured', { binding: '9+11', label: 'LB / L1 + RB / R1' }), 1500);
      return true;
    },
    gamepadCancel: async () => undefined,
    wizardInfo: async () => ({
      environment: { vs: false, cmakeUnusable: false, git: false, installer: true, missing: [], missingPackages: [], installCommand: '' },
      hasManifest: true,
      defaultDestination: 'C:\\GodOfWar-PC',
    }),
    validateIso: async (path) =>
      later(path.trim() === ''
        ? { ok: false, reason: 'empty', hash: '' }
        : /god/i.test(path)
          ? { ok: true, reason: null, hash: '33c1808ac082e236188fb887d97b7688' }
          : { ok: false, reason: 'not-found', hash: '' }),
    validateDestination: async (path) =>
      path.trim() === '' ? { ok: false, reason: 'empty', freeGb: 0 }
        : /[^\x20-\x7e]/.test(path) ? { ok: false, reason: 'characters', freeGb: 0 }
        : { ok: true, reason: null, freeGb: 412 },
    findIso: async () => {
      emit('iso-searching', true);
      setTimeout(() => {
        emit('iso-searching', false);
        emit('iso-found', 'D:\\Games\\God of War (USA).iso');
      }, 1200);
    },
    installStart: async () => {
      let stage = 0;
      const timer = setInterval(() => {
        emit('install-progress', { stage, stageProgress: 0.5, recentLines: [`==> stage ${stage}`, '[12/340] Building CXX object'] });
        if (++stage > 7) {
          clearInterval(timer);
          emit('install-done', { ok: true, cancelled: false, error: '', elf: 'C:\\GodOfWar-PC\\game\\SCUS_973.99', executable: 'C:\\GodOfWar-PC\\build\\ps2xRuntime\\gow-2005.exe' });
        }
      }, 600);
    },
    installCancel: async () => undefined,
    installComplete: async (args) => merge({ elf: args.elf, executable: args.executable, optimizationDestination: args.destination, iso: '' }),
    optimizerSnapshot: async () => snapshot(),
    optimizeNow: async () => undefined,
    on: async (event, handler) => {
      const set = listeners.get(event) ?? new Set<Handler>();
      set.add(handler as Handler);
      listeners.set(event, set);
      return () => set.delete(handler as Handler);
    },
    pick: async () => null,
    onFileDrop: async () => () => undefined,
    minimize: async () => undefined,
    toggleMaximize: async () => undefined,
    close: async () => undefined,
    forceClose: async () => undefined,
  };
}

let instance: Promise<Api> | undefined;

/** The backend for this environment: Tauri when running inside the app, the mock in a browser. */
export function getApi(): Promise<Api> {
  instance ??= isTauri
    ? createTauriApi()
    : Promise.resolve(createMockApi({ ready: new URLSearchParams(window.location.search).get('state') !== 'missing' }));
  return instance;
}

/** For tests: replaces the backend. */
export function setApi(api: Api) {
  instance = Promise.resolve(api);
}
