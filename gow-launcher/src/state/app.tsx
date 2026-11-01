import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { getApi, noUpdate, type Api } from '../lib/backend';
import { asLanguage, format, formatDuration, systemLocale, translate } from '../lib/i18n';
import { gigabytes, nextOverall } from '../lib/texturePack';
import type {
  Bootstrap, GameFinished, GameStatus, Language, LaunchError, OptimizerSnapshot, Settings, TexturePackOutcome,
  TexturePackProgress, TexturePackStatus, UpdateBlocker, UpdateState,
} from '../lib/types';

export interface ErrorNotice {
  text: string;
  detail: string;
}

/**
 * The texture pack installation. It lives here, not in the Textures page, so it survives the page being left and the
 * rest of the interface can hold still while it runs (the sections, PLAY and closing the window are blocked).
 */
export interface TextureInstall {
  /** A pack is being downloaded or extracted. */
  active: boolean;
  /** Last report of the running installation; `overall` only ever goes up. */
  progress: TexturePackProgress | null;
  /** What the last installation that finished well left installed, for the page to show. */
  installed: TexturePackStatus | null;
}

export interface AppContextValue {
  api: Api;
  info: Omit<Bootstrap, 'settings' | 'arguments'>;
  settings: Settings;
  /** Applies the change at once and saves it (debounced) on the Rust side. */
  patch: (change: Partial<Settings>) => void;
  /** Waits for pending changes to reach the Rust side. */
  flush: () => Promise<void>;
  reset: () => Promise<void>;
  lang: Language;
  t: (key: string) => string;
  f: (key: string, ...values: Array<string | number>) => string;
  duration: (seconds: number) => string;
  status: GameStatus | null;
  refreshStatus: () => Promise<void>;
  detectPaths: (force: boolean) => Promise<void>;
  gameRunning: boolean;
  optimizer: OptimizerSnapshot;
  play: () => Promise<void>;
  stop: () => Promise<void>;
  toast: string;
  notify: (text: string) => void;
  error: ErrorNotice | null;
  showError: (text: string, detail?: string) => void;
  dismissError: () => void;
  wizardOpen: boolean;
  openWizard: () => void;
  closeWizard: () => void;
  adoptSettings: (settings: Settings) => void;
  textureInstall: TextureInstall;
  /** Starts the pack installation; `expectedBytes` is the size the server announced, for the first report. */
  installTexturePack: (expectedBytes: number) => Promise<void>;
  /** The window was asked to close while the pack installs and stayed open: the player has to confirm. */
  closeBlocked: boolean;
  keepInstalling: () => void;
  closeAnyway: () => void;
  /** The newer launcher release, if any (`idle` shows nothing), and what the update would interrupt. */
  update: UpdateState;
  updateBlocker: UpdateBlocker | null;
  /** The notice's button in an installed copy: downloads, installs and restarts. */
  installUpdate: () => Promise<void>;
  /** The notice's button in a portable copy or a package install: opens the release page. */
  openUpdatePage: () => Promise<void>;
}

const Context = createContext<AppContextValue | null>(null);

export function useApp(): AppContextValue {
  const value = useContext(Context);
  if (!value) throw new Error('useApp outside of AppProvider');
  return value;
}

const idleOptimizer: OptimizerSnapshot = {
  configured: false, compiling: false, captured: 0, compiled: 0, completionRatio: 1, buildProgress: -1, status: 'disabled',
};

const idleTextureInstall: TextureInstall = { active: false, progress: null, installed: null };

const SAVE_DELAY_MS = 200;
const TOAST_MS = 4800;

export function AppProvider({ children }: { children: ReactNode }) {
  const [api, setApi] = useState<Api | null>(null);
  const [boot, setBoot] = useState<Bootstrap | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [status, setStatus] = useState<GameStatus | null>(null);
  const [gameRunning, setGameRunning] = useState(false);
  const [optimizer, setOptimizer] = useState<OptimizerSnapshot>(idleOptimizer);
  const [toast, setToast] = useState('');
  const [error, setError] = useState<ErrorNotice | null>(null);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [textureInstall, setTextureInstall] = useState<TextureInstall>(idleTextureInstall);
  const [closeBlocked, setCloseBlocked] = useState(false);
  const [update, setUpdate] = useState<UpdateState>(noUpdate);
  const [installing, setInstalling] = useState(false);
  const textureActive = useRef(false);
  textureActive.current = textureInstall.active;
  const pending = useRef<Partial<Settings>>({});
  const timer = useRef<number>(0);
  const toastTimer = useRef<number>(0);
  const settingsRef = useRef<Settings | null>(null);
  settingsRef.current = settings;

  const lang = asLanguage(settings?.language ?? 'en');
  const t = useCallback((key: string) => translate(lang, key), [lang]);
  const f = useCallback((key: string, ...values: Array<string | number>) => format(lang, key, ...values), [lang]);
  const duration = useCallback((seconds: number) => formatDuration(lang, seconds), [lang]);

  const notify = useCallback((text: string) => {
    setToast(text);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(''), TOAST_MS);
  }, []);
  const showError = useCallback((text: string, detail = '') => setError({ text, detail }), []);

  const adoptSettings = useCallback((next: Settings) => setSettings({ ...next, ...pending.current }), []);

  const refreshStatus = useCallback(async () => {
    if (api) setStatus(await api.gameStatus());
  }, [api]);

  const flush = useCallback(async () => {
    window.clearTimeout(timer.current);
    const change = pending.current;
    if (!api || Object.keys(change).length === 0) return;
    pending.current = {};
    await api.patchSettings(change);
  }, [api]);

  const patch = useCallback(
    (change: Partial<Settings>) => {
      setSettings((current) => (current ? { ...current, ...change } : current));
      pending.current = { ...pending.current, ...change };
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => void flush(), SAVE_DELAY_MS);
    },
    [flush],
  );

  // Boot: backend, migrated settings, first status.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const backend = await getApi();
      const data = await backend.bootstrap(systemLocale());
      const detected = await backend.detectPaths(false);
      if (cancelled) return;
      setApi(backend);
      setBoot(data);
      setSettings(detected);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Events from the Rust side.
  useEffect(() => {
    if (!api) return;
    const stops: Array<Promise<() => void>> = [
      api.on('settings-changed', adoptSettings),
      api.on('game-running', setGameRunning),
      api.on('optimizer', setOptimizer),
      api.on('texture-pack-progress', (event: TexturePackProgress) => {
        textureActive.current = true;
        setTextureInstall((current) => ({
          ...current,
          active: true,
          progress: { ...event, overall: nextOverall(current.progress?.overall ?? 0, event.overall) },
        }));
      }),
      api.on('texture-pack-done', (event: TexturePackOutcome) => {
        const language = asLanguage(settingsRef.current?.language ?? 'en');
        textureActive.current = false;
        setCloseBlocked(false);
        if (event.ok && event.status) {
          const status = event.status;
          setTextureInstall({ active: false, progress: null, installed: status });
          notify(format(language, 'texturePackInstalled', status.files.toLocaleString(language), gigabytes(status.bytes, language)));
          return;
        }
        setTextureInstall((current) => ({ ...current, active: false, progress: null }));
        // `noGame` is the player's situation, not a failure: say what to do, in their language, with no English detail.
        if (event.code === 'noGame') notify(translate(language, 'texturePackNeedsGame'));
        else showError(translate(language, 'texturePackFailed'), event.error ?? undefined);
      }),
      api.on('close-blocked', () => setCloseBlocked(true)),
      api.on('update-state', setUpdate),
      api.on('install-progress', () => setInstalling(true)),
      api.on('install-done', () => setInstalling(false)),
      api.on('game-finished', (finished: GameFinished) => {
        if (finished.failed) {
          showError(format(asLanguage(settingsRef.current?.language ?? 'en'), 'finishedFailure', finished.code ?? '?'), finished.summary);
        } else {
          const language = asLanguage(settingsRef.current?.language ?? 'en');
          notify(format(language, 'finishedOk', formatDuration(language, finished.seconds)));
        }
      }),
    ];
    void api.isGameRunning().then(setGameRunning);
    void api.optimizerSnapshot().then(setOptimizer);
    void api.updateState().then(setUpdate);
    void refreshStatus();
    return () => {
      stops.forEach((stop) => void stop.then((off) => off()));
    };
  }, [api, adoptSettings, notify, showError, refreshStatus]);

  useEffect(() => {
    void refreshStatus();
  }, [settings?.elf, settings?.iso, settings?.executable, refreshStatus]);

  const play = useCallback(async () => {
    if (!api) return;
    await flush();
    try {
      await api.startGame();
    } catch (failure) {
      const launch = failure as LaunchError;
      const language = asLanguage(settingsRef.current?.language ?? 'en');
      showError(format(language, launch.key, launch.detail), launch.key === 'errorArranque' ? '' : launch.detail);
    }
  }, [api, flush, showError]);

  const stop = useCallback(async () => {
    await api?.stopGame();
  }, [api]);

  const installTexturePack = useCallback(
    async (expectedBytes: number) => {
      // One at a time, whatever the page does: a second click or a page that was left and reopened.
      if (!api || textureActive.current) return;
      textureActive.current = true;
      setTextureInstall((current) => ({
        ...current,
        active: true,
        installed: null,
        progress: { stage: 'downloading', done: 0, total: expectedBytes, overall: 0 },
      }));
      try {
        await api.installTexturePack();
      } catch (failure) {
        textureActive.current = false;
        setTextureInstall((current) => ({ ...current, active: false, progress: null }));
        showError(t('texturePackFailed'), failure instanceof Error ? failure.message : String(failure));
      }
    },
    [api, showError, t],
  );

  // The same reasons the backend refuses an update for (it is the one that decides; this only explains the disabled button).
  const updateBlocker: UpdateBlocker | null = gameRunning
    ? 'game'
    : installing
      ? 'installation'
      : optimizer.compiling
        ? 'optimizer'
        : textureInstall.active
          ? 'texturePack'
          : null;

  const installUpdate = useCallback(async () => {
    if (!api) return;
    try {
      await api.updateInstall();
    } catch (failure) {
      const code = String(failure);
      if (code.startsWith('busy:')) notify(translate(asLanguage(settingsRef.current?.language ?? 'en'), `updateBlocked_${code.slice(5)}`));
      else setUpdate((current) => ({ ...current, phase: 'error', error: code }));
    }
  }, [api, notify]);

  const openUpdatePage = useCallback(async () => {
    await api?.updateOpenRelease();
  }, [api]);

  const keepInstalling = useCallback(() => setCloseBlocked(false), []);
  const closeAnyway = useCallback(() => {
    setCloseBlocked(false);
    void api?.forceClose();
  }, [api]);

  const detectPaths = useCallback(
    async (force: boolean) => {
      if (api) adoptSettings(await api.detectPaths(force));
    },
    [api, adoptSettings],
  );

  const reset = useCallback(async () => {
    if (!api) return;
    pending.current = {};
    adoptSettings(await api.resetGameOptions());
  }, [api, adoptSettings]);

  // "--play" and "--wizard" come from the shortcut; a missing game opens the wizard once.
  const started = useRef(false);
  useEffect(() => {
    if (started.current || !api || !boot || !settings || !status) return;
    started.current = true;
    if (!status.ready || boot.arguments.wizard) setWizardOpen(true);
    else if (boot.arguments.play) void play();
  }, [api, boot, settings, status, play]);

  const value = useMemo<AppContextValue | null>(() => {
    if (!api || !boot || !settings) return null;
    const { settings: _initial, arguments: _arguments, ...info } = boot;
    void _initial;
    void _arguments;
    return {
      api, info, settings, patch, flush, reset, lang, t, f, duration, status, refreshStatus, detectPaths, gameRunning,
      optimizer, play, stop, toast, notify, error, showError,
      dismissError: () => setError(null), wizardOpen, openWizard: () => setWizardOpen(true),
      closeWizard: () => setWizardOpen(false), adoptSettings,
      textureInstall, installTexturePack, closeBlocked, keepInstalling, closeAnyway,
      update, updateBlocker, installUpdate, openUpdatePage,
    };
  }, [
    api, boot, settings, patch, flush, reset, lang, t, f, duration, status, refreshStatus, detectPaths, gameRunning,
    optimizer, play, stop, toast, notify, error, showError, wizardOpen, adoptSettings,
    textureInstall, installTexturePack, closeBlocked, keepInstalling, closeAnyway,
    update, updateBlocker, installUpdate, openUpdatePage,
  ]);

  if (!value) return null;
  return <Context.Provider value={value}>{children}</Context.Provider>;
}
