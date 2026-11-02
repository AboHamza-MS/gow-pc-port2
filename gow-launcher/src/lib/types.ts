// Shapes exchanged with the Rust side (`gow-core`, serde camelCase). Keep in sync with the crate.

export type Language = 'en' | 'pt-BR';

export interface Settings {
  language: string;
  gpu: string;
  displayMode: 'fullscreen' | 'window' | string;
  widthPx: number;
  heightPx: number;
  aspectMode: string;
  /** Widescreen switch: the PCSX2 patch widens the camera (Hor+) at `aspectMode`; off keeps the original 4:3 picture. */
  widescreen: boolean;
  stretch: boolean;
  filter: 'fsr' | 'bilinear' | 'pixel' | string;
  sharpness: number;
  fxaa: boolean;
  progressive480p: boolean;
  internalResolution: string;
  fpsLimit: number;
  vsync: boolean;
  showFps: boolean;
  elf: string;
  iso: string;
  executable: string;
  skipVideos: boolean;
  fastBoot: boolean;
  customTextures: boolean;
  asyncTextures: boolean;
  settingsVersion: number;
  dmc5Controls: boolean;
  padButtons: string;
  cancelAnimations: boolean;
  escQuits: boolean;
  minimizeOnPlay: boolean;
  secondsPlayed: number;
  lastSession: number;
  keys: string;
  cheats: string;
  cheatsShortcut: string;
  retroAchievementsEnabled: boolean;
  retroAchievementsUser: string;
  retroAchievementsHardcore: boolean;
  retroAchievementsNotifications: boolean;
  optimizationDestination: string;
  optimizationEnabled: boolean;
  optimizeDuringGame: boolean;
  optimizationCompiled: number;
  /** Install a newer launcher at start-up when nothing is running (off: only the notice). */
  autoUpdate: boolean;
}

export interface DisplayInfo {
  widthPx: number;
  heightPx: number;
  hz: number;
}

export interface GraphicsCard {
  id: string;
  kind: string;
  displayName: string;
  manufacturer: string;
  driver: string;
  isDefault: boolean;
}

export interface Bootstrap {
  settings: Settings;
  version: string;
  platform: 'windows' | 'linux';
  display: DisplayInfo;
  graphicsCards: GraphicsCard[];
  dataDirectory: string;
  arguments: { play: boolean; wizard: boolean };
}

export interface GameState {
  exists: boolean;
  valid: boolean;
  modified: boolean;
  crc: string;
}
export interface DataState {
  ok: boolean;
  iso: string;
  extracted: boolean;
  chosenIsoMissing: boolean;
}
export interface ExecutableState {
  exists: boolean;
  executable: boolean;
}
export interface GameStatus {
  game: GameState;
  data: DataState;
  executable: ExecutableState;
  ready: boolean;
}

export interface GameFinished {
  code: number | null;
  failed: boolean;
  seconds: number;
  summary: string;
  /** The i18n key that explains a recognized failure (`failureOpenGl`...), shown above the raw summary; null if unknown. */
  failureKind: string | null;
}

export interface LaunchError {
  key: string;
  detail: string;
}

export type OptimizerState = 'disabled' | 'idle' | 'compiling' | 'error';
export interface OptimizerSnapshot {
  configured: boolean;
  compiling: boolean;
  captured: number;
  compiled: number;
  completionRatio: number;
  buildProgress: number;
  status: OptimizerState;
}

export interface WizardInfo {
  environment: {
    vs: boolean;
    /** Windows: there is a Visual Studio but no usable CMake (3.21 or newer, with cmcldeps.exe) in PATH or bundled with it.
     * Informational: the installer downloads a portable CMake in that case. */
    cmakeUnusable: boolean;
    git: boolean;
    installer: boolean;
    /** Linux: programs and libraries the build needs and the system lacks (empty on Windows or when all is there). */
    missing: string[];
    /** Linux: the packages that provide them, for this distribution (empty if it is not a known one). */
    missingPackages: string[];
    /** Linux: the command that installs those packages, to run in a terminal. */
    installCommand: string;
    /** Windows x86-64: CPU features the game needs that this processor lacks (`['SSE4.1']`); empty when all are there. */
    cpuMissing: string[];
    /** Installation kit files (relative to the kit folder) that are gone and could not be restored; empty when all are there. */
    kitMissing: string[];
  };
  hasManifest: boolean;
  defaultDestination: string;
}

export type IsoReason = 'empty' | 'not-found' | 'unreadable' | 'format' | 'other-game' | 'no-executable' | 'modified';
export interface IsoValidation {
  ok: boolean;
  reason: IsoReason | null;
  hash: string;
}

export type DestinationReason = 'empty' | 'characters' | 'space';
export interface DestinationValidation {
  ok: boolean;
  reason: DestinationReason | null;
  freeGb: number;
}

export interface InstallProgress {
  stage: number;
  stageProgress: number;
  recentLines: string[];
}
export interface InstallOutcome {
  ok: boolean;
  cancelled: boolean;
  error: string;
  elf: string;
  executable: string;
  /** The i18n key that explains a recognized failure (`failureDownload`...), shown above the raw error; null if unknown. */
  failureKind: string | null;
}

export interface GamepadCaptured {
  binding: string;
  label: string;
}

/** Size, ETag and upload date of the community texture pack (its `_meta` endpoint). */
export interface TexturePackMeta {
  size: number;
  etag: string;
  uploaded: string;
}

/** What the texture page reads: the pack online and what is installed now. */
export interface TexturePackInfo {
  meta: TexturePackMeta | null;
  status: TexturePackStatus;
}

export interface TexturePackStatus {
  installed: boolean;
  files: number;
  bytes: number;
  etag: string;
  uploaded: string;
}

export interface TexturePackProgress {
  stage: 'downloading' | 'extracting';
  /** Bytes downloaded, or files extracted. */
  done: number;
  /** Content length, or number of files to extract (0 when unknown). */
  total: number;
  /** Progress of the whole installation, 0 to 1: the download first, then the extraction, one bar that never goes back. */
  overall: number;
}

export interface TexturePackOutcome {
  ok: boolean;
  status: TexturePackStatus | null;
  /** English detail, for the log/dialog body. */
  error: string | null;
  /** Stable code the interface translates (`noGame`, `noSpace`, `network`...). */
  code: string | null;
}

/** Nothing to show (no newer release, or the check failed), a release to offer, or the update running or failed. */
export type UpdatePhase = 'idle' | 'available' | 'downloading' | 'installing' | 'error';

/** What the button does: update in place, or open the release page (the portable zip, a package install, an unsigned release). */
export type UpdateMode = 'automatic' | 'portable' | 'manual';

/** Event `update-state` and command `update_state`. */
export interface UpdateState {
  phase: UpdatePhase;
  mode: UpdateMode;
  version: string;
  pageUrl: string;
  /** Bytes downloaded and announced size (0 while unknown). */
  done: number;
  total: number;
  /** English detail of the last failure, for the tooltip. */
  error: string;
}

/** What an update must not interrupt; the backend refuses with `busy:<code>` for the same reasons. */
export type UpdateBlocker = 'game' | 'installation' | 'optimizer' | 'texturePack';
