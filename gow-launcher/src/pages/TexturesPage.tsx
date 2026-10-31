import { useEffect, useState } from 'react';
import { Download, FolderOpen, Layers, Zap } from 'lucide-react';
import { PrimaryButton, SecondaryButton, SettingRow, StatusLine, Switch } from '../components/ui';
import { gigabytes } from '../lib/texturePack';
import { useApp } from '../state/app';
import type { TexturePackInfo } from '../lib/types';

/**
 * What the page shows in place of the pack card: nothing, a request to install the game first, or the card itself.
 * The card is only offered while custom textures are on, the game files are set and nothing is installed yet — once the
 * folder has files the card goes away, as the pack has already been used.
 */
export type PackState = 'hidden' | 'ask' | 'offer';

export function packState(customTextures: boolean, info: TexturePackInfo | null, gameReady: boolean): PackState {
  if (!customTextures || info === null || info.status.installed) return 'hidden';
  return gameReady ? 'offer' : 'ask';
}

export function TexturesPage() {
  const { settings, patch, api, notify, lang, status: gameStatus, textureInstall, installTexturePack, t, f } = useApp();
  const [info, setInfo] = useState<TexturePackInfo | null>(null);
  // The installation lives in the app state: leaving this page cannot lose it, and while it runs nothing else is reachable.
  const installing = textureInstall.active;
  const progress = textureInstall.progress;

  useEffect(() => {
    let cancelled = false;
    void api
      .texturePackInfo()
      .then((pack) => {
        if (!cancelled) setInfo(pack);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [api]);

  // An installation that finishes leaves its result in the app state.
  useEffect(() => {
    const status = textureInstall.installed;
    if (status) setInfo((current) => ({ meta: current?.meta ?? null, status }));
  }, [textureInstall.installed]);

  const openTextures = async () => {
    if (!(await api.openTexturesFolder())) notify(t('texturesNeedGame'));
  };

  const install = () => {
    if (installing) return;
    if (!gameReady) {
      notify(t('texturePackNeedsGame'));
      return;
    }
    void installTexturePack(info?.meta?.size ?? 0);
  };

  const gameReady = Boolean(gameStatus?.ready);
  const state = packState(settings.customTextures, info, gameReady);
  const offerPack = state === 'offer';
  const askForGame = state === 'ask';
  const size = info?.meta ? gigabytes(info.meta.size, lang) : '?';
  const uploaded = info?.meta?.uploaded ? new Date(info.meta.uploaded).toLocaleDateString(lang) : '';

  return (
    <div className="flex flex-col gap-1">
      <SettingRow icon={Layers} title={t('customTextures')} description={t('customTexturesDesc')}>
        <Switch label={t('customTextures')} checked={settings.customTextures} disabled={installing} onChange={(customTextures) => patch({ customTextures })} />
      </SettingRow>
      {installing ? (
        <div className="px-1 pb-2">
          <StatusLine severity="warning">{t('texturePackBusy')}</StatusLine>
        </div>
      ) : null}
      {askForGame ? (
        <div className="px-1 pb-2">
          <StatusLine severity="warning">{t('texturePackNeedsGame')}</StatusLine>
        </div>
      ) : null}
      {offerPack ? (
        <section
          aria-label={t('texturePack')}
          className="relative my-2.5 overflow-hidden rounded-[8px] border border-line-light bg-[linear-gradient(180deg,#f6e8ca_0%,#e9d6b0_58%,#dcc9a2_100%)]"
        >
          <span className="pointer-events-none absolute inset-x-0 top-0 h-[3px] bg-[linear-gradient(90deg,#5a0f09,#b3241a_50%,#5a0f09)]" />
          <span className="pointer-events-none absolute inset-[4px] rounded-[6px] border border-gold/30" />
          <div className="flex items-start gap-3 px-4 pb-1.5 pt-4">
            <span className="mt-0.5 flex h-10 w-10 shrink-0 items-center justify-center rounded-full border border-gold/50 bg-cream/85 text-accent">
              <Download size={19} strokeWidth={1.9} />
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2">
                <span className="gow-text-shadow text-[13.5px] font-bold uppercase tracking-[0.13em] text-ink">{t('texturePack')}</span>
                <span className="rounded-[3px] border border-accent/50 bg-accent/10 px-1.5 py-px text-[10px] font-bold uppercase tracking-[0.14em] text-accent">
                  {t('texturePackBadge')}
                </span>
              </div>
              <p className="mt-1.5 text-[12.5px] font-semibold leading-[1.35] text-ink-muted">{f('texturePackDesc', size)}</p>
            </div>
          </div>
          <div className="gow-rule mx-4 mt-1 opacity-75" />
          <div className="flex items-center justify-between gap-3 px-4 pb-3 pt-2">
            <span className="text-[12px] font-semibold text-ink-muted">{uploaded ? f('texturePackUpdated', uploaded) : ''}</span>
            <PrimaryButton disabled={installing} onClick={install}>
              {installing ? t('texturePackWorking') : t('texturePackDownload')}
            </PrimaryButton>
          </div>
          {progress ? (
            <div className="flex items-center gap-3 px-4 pb-3">
              <div
                className="gow-track h-[10px] flex-1"
                role="progressbar"
                aria-label={t('texturePack')}
                aria-valuenow={Math.round(progress.overall * 100)}
                aria-valuemin={0}
                aria-valuemax={100}
              >
                <div
                  className="gow-track-fill h-full min-w-1 transition-[width] duration-300 ease-out"
                  style={{ width: `${progress.overall * 100}%` }}
                />
              </div>
              <span className="shrink-0 text-[12px] font-semibold text-ink-muted">
                {progress.stage === 'downloading'
                  ? progress.total > 0
                    ? f('texturePackDownloading', gigabytes(progress.done, lang), gigabytes(progress.total, lang))
                    : t('texturePackWorking')
                  : f('texturePackExtracting', progress.done, progress.total)}
                {' · '}
                {Math.round(progress.overall * 100)} %
              </span>
            </div>
          ) : null}
        </section>
      ) : null}
      <SettingRow disabled={!settings.customTextures} icon={Zap} title={t('asyncTextures')} description={t('asyncTexturesDesc')}>
        <Switch label={t('asyncTextures')} checked={settings.asyncTextures} onChange={(asyncTextures) => patch({ asyncTextures })} />
      </SettingRow>
      <SettingRow disabled={!settings.customTextures} icon={FolderOpen} title={t('texturesFolder')} description={t('texturesFolderDesc')}>
        <SecondaryButton disabled={installing} onClick={() => void openTextures()}>{t('open')}</SecondaryButton>
      </SettingRow>
    </div>
  );
}
