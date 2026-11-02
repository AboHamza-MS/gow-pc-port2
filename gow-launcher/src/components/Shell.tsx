import { Gauge, Gamepad2, FolderOpen, Layers, Minus, Monitor, Play, Settings2, Sparkles, Square, Trophy, X } from 'lucide-react';
import badgeUrl from '../assets/theme/badge.webp';
import logoUrl from '../assets/theme/logo.webp';
import { useApp } from '../state/app';
import { MarkField, useReportExport } from './ReportTools';
import { SecondaryButton, type IconComponent } from './ui';

// ----------------------------------------------------------------------------------------------- title bar

export function TitleBar() {
  const { api, t } = useApp();
  return (
    <header
      data-tauri-drag-region
      className="relative z-[60] flex h-[34px] shrink-0 items-center justify-between border-b border-[#5a3d22] bg-gradient-to-r from-[#17100b]/90 via-[#2c1d12]/80 to-[#3a1d14]/70"
    >
      <div data-tauri-drag-region className="flex items-center gap-2.5 pl-3">
        <img src={badgeUrl} alt="" width={22} height={22} className="pointer-events-none" />
        <span data-tauri-drag-region className="text-[11px] font-semibold tracking-[0.18em] text-[#d9c6a3]">
          GOD OF WAR (2005)
        </span>
      </div>
      {/* Same place in every section, and clear of the window controls: the title bar is the one strip that never changes. */}
      <div data-tauri-drag-region className="flex h-full min-w-0 flex-1 items-center justify-center px-4">
        <UpdateNotice />
      </div>
      <div className="flex h-full">
        {[
          { id: 'minimize', icon: Minus, size: 16, run: () => api.minimize() },
          { id: 'maximize', icon: Square, size: 12, run: () => api.toggleMaximize() },
          { id: 'close', icon: X, size: 16, run: () => api.close() },
        ].map(({ id, icon: Icon, size, run }) => (
          <button
            key={id}
            type="button"
            title={t(id)}
            aria-label={t(id)}
            onClick={() => void run()}
            className={`flex h-full w-[46px] items-center justify-center text-[#e7d6b4] transition-colors ${id === 'close' ? 'hover:bg-[#a3190f]' : 'hover:bg-white/10'}`}
          >
            <Icon size={size} />
          </button>
        ))}
      </div>
    </header>
  );
}

// ----------------------------------------------------------------------------------------------- update notice

const noticeButton =
  'h-[22px] shrink-0 whitespace-nowrap rounded border border-gold bg-[#4a2f18] px-2.5 text-[11px] font-semibold tracking-wide text-cream transition-colors hover:bg-[#6a4424] disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:bg-[#4a2f18]';

/**
 * A newer launcher release, in the title bar. It says what the button will do (update here, or open the release page for
 * a copy that cannot update itself), shows the progress while it runs and a failure with a retry, and explains a disabled
 * button instead of just greying it. Nothing at all while there is nothing to offer.
 */
export function UpdateNotice() {
  const { update, updateBlocker, installUpdate, openUpdatePage, f, t } = useApp();
  if (update.phase === 'idle') return null;
  const { phase, mode, version } = update;
  const text = 'min-w-0 truncate text-[11px] font-semibold tracking-wide text-[#e7d6b4]';
  const muted = 'min-w-0 truncate text-[11px] text-[#b9a27a]';

  if (phase === 'downloading' || phase === 'installing') {
    const known = update.total > 0;
    const percent = phase === 'installing' ? 100 : known ? Math.min(100, Math.floor((update.done / update.total) * 100)) : 0;
    const label = phase === 'installing' ? t('updateInstalling') : known ? f('updateDownloading', percent) : t('updateDownloadingUnknown');
    return (
      <div role="status" className="flex min-w-0 items-center gap-3" title={label}>
        <span className={text}>{label}</span>
        <div
          role="progressbar"
          aria-label={t('updateProgress')}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={known || phase === 'installing' ? percent : undefined}
          className="h-[6px] w-[150px] shrink-0 overflow-hidden rounded-full bg-[#140b06]/70"
        >
          <div
            className={`h-full rounded-full bg-gold-light ${known || phase === 'installing' ? '' : 'w-1/3 animate-pulse'}`}
            style={known || phase === 'installing' ? { width: `${percent}%` } : undefined}
          />
        </div>
      </div>
    );
  }

  if (phase === 'error' && mode === 'automatic') {
    return (
      <div role="status" className="flex min-w-0 items-center gap-3" title={update.error}>
        <span className={`${text} text-[#f0b4a8]`}>{f('updateFailed', version)}</span>
        <button type="button" className={noticeButton} disabled={updateBlocker !== null} onClick={() => void installUpdate()}>
          {t('updateRetry')}
        </button>
        {updateBlocker && <span className={muted}>{t(`updateBlocked_${updateBlocker}`)}</span>}
      </div>
    );
  }

  if (mode === 'automatic') {
    const reason = updateBlocker ? t(`updateBlocked_${updateBlocker}`) : '';
    return (
      <div role="status" className="flex min-w-0 items-center gap-3" title={reason}>
        <span className={text}>{f('updateAvailable', version)}</span>
        {reason && <span className={muted}>{reason}</span>}
        <button type="button" className={noticeButton} disabled={updateBlocker !== null} onClick={() => void installUpdate()}>
          {t('updateButton')}
        </button>
      </div>
    );
  }

  // A portable copy (or a package install) cannot replace itself: the button opens the page where the new one is.
  const why = t(mode === 'portable' ? 'updatePortableNote' : 'updateManualNote');
  return (
    <div role="status" className="flex min-w-0 items-center gap-3" title={`${f('updateAvailable', version)}. ${why}`}>
      <span className={text}>{f('updateAvailable', version)}</span>
      <span className={muted}>{why}</span>
      <button type="button" className={noticeButton} onClick={() => void openUpdatePage()}>
        {t('updateOpenPage')}
      </button>
    </div>
  );
}

// ----------------------------------------------------------------------------------------------- hero

/** Small parchment label for the information that sits over the banner art, so it stays legible on any part of it. */
const chip = 'whitespace-nowrap rounded-md border border-line bg-[#eadbb9]/95 px-3 py-1 shadow-[0_1px_3px_rgba(40,20,5,0.35)]';

export function Hero() {
  const { status, gameRunning, play, stop, openWizard, textureInstall, t } = useApp();
  const ready = status?.ready ?? false;
  // The textures being written are the ones the game would read.
  const installingTextures = textureInstall.active;
  return (
    <section className="relative flex h-[204px] shrink-0 items-stretch justify-between gap-[22px] overflow-hidden pl-[72px] pr-[30px] pb-2 pt-3">
      <div className="gow-glow relative flex min-w-0 flex-col justify-center">
        <img src={logoUrl} alt="God of War" width={430} className="h-auto w-[430px] max-w-full drop-shadow-[0_3px_3px_rgba(40,15,5,0.4)]" />
        {/* Out of the flow, so the logo stays centred in the banner. */}
        <div className="absolute bottom-1.5 left-0 flex w-max items-end gap-2">
          {gameRunning && (
            <button type="button" onClick={() => void stop()} className={`${chip} text-[12.5px] font-semibold text-ink hover:text-accent`}>
              {t('closeGame')}
            </button>
          )}
        </div>
      </div>
      <button
        type="button"
        disabled={gameRunning || installingTextures}
        onClick={() => (ready ? void play() : openWizard())}
        className="gow-plaque relative mb-1 flex h-[72px] w-[300px] shrink-0 items-center justify-center gap-3 self-end font-title text-[24px] font-bold tracking-[0.14em] text-cream"
      >
        {ready ? <Play size={22} fill="currentColor" /> : <Sparkles size={22} />}
        {gameRunning ? t('running') : installingTextures ? t('texturePackPlayBlocked') : ready ? t('play') : t('wizInstall')}
      </button>
    </section>
  );
}

// ----------------------------------------------------------------------------------------------- sections

export type SectionId = 'graphics' | 'textures' | 'performance' | 'files' | 'controls' | 'cheats' | 'other' | 'retro';

export const sections: { id: SectionId; key: string; icon: IconComponent }[] = [
  { id: 'graphics', key: 'tabGraphics', icon: Monitor },
  { id: 'textures', key: 'tabTextures', icon: Layers },
  { id: 'performance', key: 'tabPerformance', icon: Gauge },
  { id: 'files', key: 'tabFiles', icon: FolderOpen },
  { id: 'controls', key: 'tabControls', icon: Gamepad2 },
  { id: 'cheats', key: 'tabCheats', icon: Sparkles },
  { id: 'other', key: 'tabOther', icon: Settings2 },
  { id: 'retro', key: 'tabRetroAchievements', icon: Trophy },
];

export function NavRail({ section, onSelect }: { section: SectionId; onSelect: (id: SectionId) => void }) {
  const { t, textureInstall } = useApp();
  // Installing the texture pack takes minutes and its progress belongs to the Textures section: nothing else is reachable.
  const locked = textureInstall.active;
  return (
    <nav className="relative flex w-[252px] shrink-0 flex-col" aria-label={t('navSections')}>
      <div className="gow-fill gow-fill-fade" />
      <div className="gow-frame" />
      <div className="relative flex flex-col gap-1 px-[14px] pt-[26px]">
        {sections.map(({ id, key, icon: Icon }) => {
          const active = id === section;
          const blocked = locked && !active;
          return (
            <button
              key={id}
              type="button"
              title={blocked ? t('texturePackLocked') : t(key)}
              aria-label={t(key)}
              aria-current={active ? 'page' : undefined}
              disabled={blocked}
              onClick={() => onSelect(id)}
              className={`relative flex h-[46px] items-center gap-3 rounded-[5px] pl-[30px] text-left font-serif text-[16px] font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
                active ? 'gow-plaque text-cream [--end:36px] [--pad:4px]' : blocked ? 'text-ink' : 'text-ink hover:bg-[#6f4a22]/15'
              }`}
            >
              <Icon size={22} strokeWidth={1.7} className={active ? 'text-cream' : 'text-ink-soft'} />
              <span className="truncate">{t(key)}</span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}

// ----------------------------------------------------------------------------------------------- overlays

export function Toast() {
  const { toast } = useApp();
  return (
    <div
      role="status"
      aria-live="polite"
      className={`pointer-events-none fixed bottom-[30px] left-1/2 z-20 -translate-x-1/2 rounded-full border-2 border-gold bg-[#241710]/95 px-[22px] py-2.5 text-sm font-semibold text-cream shadow-lg transition-opacity duration-300 ${
        toast ? 'opacity-100' : 'opacity-0'
      }`}
    >
      {toast}
    </div>
  );
}

export function Modal({ children, width = 560 }: { children: React.ReactNode; width?: number }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[#140b06]/65 p-6" role="dialog" aria-modal="true">
      <div className="gow-panel max-h-full overflow-y-auto" style={{ width: width + 60 }}>
        <div className="p-1">{children}</div>
      </div>
    </div>
  );
}

/**
 * The window was asked to close while a texture pack installs (the backend kept it open). Closing now throws the
 * download away, so the player decides.
 */
export function CloseBlockedDialog() {
  const { closeBlocked, textureInstall, keepInstalling, closeAnyway, t } = useApp();
  if (!closeBlocked || !textureInstall.active) return null;
  return (
    <Modal>
      <div className="flex flex-col gap-3.5">
        <h2 className="gow-text-shadow font-title text-[18px] font-bold uppercase tracking-wide text-ink">{t('texturePackCloseTitle')}</h2>
        <p className="text-[15px] font-medium text-ink">{t('texturePackCloseText')}</p>
        <div className="flex justify-end gap-2">
          <SecondaryButton danger onClick={closeAnyway}>{t('texturePackCloseAnyway')}</SecondaryButton>
          <SecondaryButton onClick={keepInstalling}>{t('texturePackKeep')}</SecondaryButton>
        </div>
      </div>
    </Modal>
  );
}

export function ErrorDialog() {
  const { error, dismissError, api, t } = useApp();
  const report = useReportExport();
  if (!error) return null;
  return (
    <Modal>
      <div className="flex flex-col gap-3.5">
        <p className="text-[15px] font-medium text-ink">{error.text}</p>
        {error.hint && <p className="text-[13px] text-ink-soft">{error.hint}</p>}
        {error.detail && (
          <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded border border-line bg-field/80 p-2.5 font-mono text-xs text-ink-soft">
            {error.detail}
          </pre>
        )}
        <MarkField />
        <div className="flex justify-end gap-2">
          <SecondaryButton onClick={() => void api.openLog()}>{t('viewLog')}</SecondaryButton>
          <SecondaryButton disabled={report.busy} onClick={() => void report.run()}>{t('reportExportButton')}</SecondaryButton>
          <SecondaryButton onClick={dismissError}>{t('gotIt')}</SecondaryButton>
        </div>
      </div>
    </Modal>
  );
}
