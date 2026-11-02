// The only module that talks to the Rust side. In a plain browser (`npm run dev`, tests, screenshots) a small
// in-memory mock stands in, so the interface can be developed and checked without the Tauri shell.

import type {
  Bootstrap, DestinationValidation, GameFinished, GameStatus, GamepadCaptured, InstallOutcome,
  InstallProgress, IsoValidation, LiveState, OptimizerSnapshot, PadDeviceList, RebuildState, Settings, TexturePackInfo, TexturePackOutcome, TexturePackProgress,
  UpdateMode, UpdateState, WizardInfo,
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
  'update-state': UpdateState;
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
  /** Writes the session report ZIP, opens its folder and returns the file name. */
  reportExport(): Promise<string>;
  /** Stores a note about a problem for the next report; false when the text was empty. */
  reportMark(text: string): Promise<boolean>;
  openDirectory(path: string): Promise<void>;
  openTexturesFolder(): Promise<boolean>;
  texturePackInfo(): Promise<TexturePackInfo>;
  installTexturePack(): Promise<void>;
  createShortcut(desktop: boolean): Promise<boolean>;
  raHasCredential(user: string): Promise<boolean>;
  raSaveCredential(user: string, password: string): Promise<boolean>;
  raClearCredential(): Promise<boolean>;
  /** The joysticks the game saw the last time it ran; null before the first run. */
  padDevices(): Promise<PadDeviceList | null>;
  /** The raw input of the joysticks right now, from the game; null when it has not written it (the game is not running). */
  padLive(): Promise<LiveState | null>;
  /** The mapping dialog asks the game for the live feed (true, repeated every few seconds) or stops asking (false). */
  padProbe(active: boolean): Promise<void>;
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
  rebuildState(): Promise<RebuildState>;
  /** Starts the rebuild of the game from the kit's sources. Rejects with a code (`busy:game`...) when it must not start. */
  rebuildStart(): Promise<void>;
  updateState(): Promise<UpdateState>;
  /** Downloads and installs the update, then the launcher restarts. Rejects with a code (`busy:game`...) when it must not start. */
  updateInstall(): Promise<void>;
  updateOpenRelease(): Promise<boolean>;
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
    reportExport: () => call('report_export'),
    reportMark: (text) => call('report_mark', { text }),
    openDirectory: (path) => call('open_directory', { path }),
    openTexturesFolder: () => call('open_textures_folder'),
    texturePackInfo: () => call('texture_pack_info'),
    installTexturePack: () => call('texture_pack_install'),
    createShortcut: (desktop) => call('create_shortcut', { desktop }),
    raHasCredential: (user) => call('ra_has_credential', { user }),
    raSaveCredential: (user, password) => call('ra_save_credential', { user, password }),
    raClearCredential: () => call('ra_clear_credential'),
    padDevices: () => call('pad_devices'),
    padLive: () => call('pad_live'),
    padProbe: (active) => call('pad_probe', { active }),
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
    rebuildState: () => call('rebuild_state'),
    rebuildStart: () => call('rebuild_start'),
    updateState: () => call('update_state'),
    updateInstall: () => call('update_install'),
    updateOpenRelease: () => call('update_open_release'),
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
  settingsVersion: 0, dmc5Controls: false, padButtons: '', padMap: '', padDevice: '', padDeviceName: '', padSdlMap: '', cancelAnimations: true, escQuits: false,
  minimizeOnPlay: true, secondsPlayed: 0, lastSession: 0, keys: '', cheats: '', cheatsShortcut: '',
  retroAchievementsEnabled: false, retroAchievementsUser: '',
  retroAchievementsHardcore: false, retroAchievementsNotifications: true, optimizationDestination: '',
  optimizationEnabled: true, optimizeDuringGame: true, optimizationCompiled: 0,
  autoUpdate: false,
};

export const noUpdate: UpdateState = { phase: 'idle', mode: 'automatic', version: '', pageUrl: '', done: 0, total: 0, error: '' };

/** An update the mock backend offers, for development in a browser (`?update=automatic`) and for tests. */
export function availableUpdate(mode: UpdateMode = 'automatic'): UpdateState {
  return { ...noUpdate, phase: 'available', mode, version: '0.2.0', pageUrl: 'https://github.com/danielalves96/gow-pc-port/releases/tag/launcher-v0.2.0' };
}

type Handler = (payload: never) => void;

export type MockApi = Api & { emit: <K extends keyof EventMap>(event: K, payload: EventMap[K]) => void };

/** What the mock backend lists as the last session's joysticks: the 2.4G dongle of issue #12, whose real pad has no SDL mapping. */
export const dongleDevices: PadDeviceList = {
  choice: 'automatic',
  active: 0,
  seenAt: 1_790_000_000,
  devices: [
    { index: 0, name: '2.4G Consumer Control', guid: '0300aaaa5e040000a102000000010000', mapped: true, buttons: 1, hats: 0, axes: 0 },
    { index: 1, name: '2.4G Keyboard', guid: '0300bbbb5e040000a102000000010000', mapped: true, buttons: 1, hats: 0, axes: 0 },
    { index: 3, name: 'Generic X-Box pad', guid: '0300dddd5e040000a102000000010000', mapped: false, buttons: 15, hats: 1, axes: 6 },
  ],
};

export function createMockApi(
  options: { ready?: boolean; update?: UpdateState; rebuildNeeded?: boolean; padDevices?: PadDeviceList | null } = {},
): MockApi {
  const ready = options.ready ?? true;
  let update = options.update ?? noUpdate;
  let rebuild: RebuildState = {
    needed: options.rebuildNeeded ?? false, kit: '3fa9c1d20b7e', built: options.rebuildNeeded ? '9b1e44c07a52' : '3fa9c1d20b7e', announce: false,
  };
  let announced = false;
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
      settings, version: '0.2.4', platform: 'windows', display,
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
        emit('game-finished', { code: 0, failed: false, seconds: 125, summary: '', failureKind: null });
      }, 2500);
    },
    stopGame: async () => undefined,
    isGameRunning: async () => running,
    openLog: async () => undefined,
    openInstallLog: async () => undefined,
    openOptimizerLog: async () => undefined,
    reportExport: async () => 'gow-report-20260101-120000.zip',
    reportMark: async (text) => text.trim().length > 0,
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
    padDevices: async () => (options.padDevices === undefined ? dongleDevices : options.padDevices),
    padLive: async () => null,
    padProbe: async () => undefined,
    gamepadBegin: async () => {
      setTimeout(() => emit('gamepad-captured', { binding: '9+11', label: 'LB / L1 + RB / R1' }), 1500);
      return true;
    },
    gamepadCancel: async () => undefined,
    wizardInfo: async () => ({
      environment: { vs: false, cmakeUnusable: false, git: false, installer: true, missing: [], missingPackages: [], installCommand: '', cpuMissing: [], kitMissing: [] },
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
          emit('install-done', { ok: true, cancelled: false, error: '', elf: 'C:\\GodOfWar-PC\\game\\SCUS_973.99', executable: 'C:\\GodOfWar-PC\\build\\ps2xRuntime\\gow-2005.exe', failureKind: null });
        }
      }, 600);
    },
    installCancel: async () => undefined,
    installComplete: async (args) => merge({ elf: args.elf, executable: args.executable, optimizationDestination: args.destination, iso: '' }),
    optimizerSnapshot: async () => snapshot(),
    optimizeNow: async () => undefined,
    rebuildState: async () => {
      const current = { ...rebuild, announce: rebuild.needed && !announced };
      announced = announced || rebuild.needed;
      return current;
    },
    rebuildStart: async () => {
      rebuild = { needed: false, kit: rebuild.kit, built: rebuild.kit, announce: false };
    },
    updateState: async () => update,
    updateInstall: async () => {
      // Short fake run: the real one downloads about 200 MB, installs and restarts the launcher.
      const publish = (change: Partial<UpdateState>) => {
        update = { ...update, ...change };
        emit('update-state', update);
      };
      publish({ phase: 'downloading', done: 0, total: 200_000_000, error: '' });
      setTimeout(() => publish({ done: 90_000_000 }), 150);
      setTimeout(() => publish({ phase: 'installing', done: 200_000_000 }), 450);
    },
    updateOpenRelease: async () => true,
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

function mockUpdateFromAddress(value: string | null): UpdateState {
  return value === 'automatic' || value === 'portable' || value === 'manual' ? availableUpdate(value) : noUpdate;
}

let instance: Promise<Api> | undefined;

/** The backend for this environment: Tauri when running inside the app, the mock in a browser. */
export function getApi(): Promise<Api> {
  instance ??= isTauri
    ? createTauriApi()
    : Promise.resolve(
        createMockApi({
          ready: new URLSearchParams(window.location.search).get('state') !== 'missing',
          update: mockUpdateFromAddress(new URLSearchParams(window.location.search).get('update')),
          rebuildNeeded: new URLSearchParams(window.location.search).get('rebuild') === 'needed',
        }),
      );
  return instance;
}

/** For tests: replaces the backend. */
export function setApi(api: Api) {
  instance = Promise.resolve(api);
}
