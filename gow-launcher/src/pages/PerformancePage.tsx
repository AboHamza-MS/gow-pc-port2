import { useMemo } from 'react';
import { Activity, Cpu, Gauge, Hourglass, MemoryStick, RefreshCw, Rocket } from 'lucide-react';
import { GroupTitle, Note, SecondaryButton, Select, SettingRow, Switch, type Option } from '../components/ui';
import { useApp } from '../state/app';

/** Internal scale requested: "auto" and anything above 6x fall back to 2x (mirrors `Settings::selected_scale`). */
export function selectedScale(internalResolution: string): number {
  const n = parseFloat(internalResolution);
  return internalResolution === 'auto' || Number.isNaN(n) || n > 6 ? 2 : Math.max(1, n);
}

export function PerformancePage() {
  const { settings, patch, optimizer, info, api, t, f } = useApp();
  const optimizing = settings.optimizationDestination.length > 0;
  const { graphicsCards, platform } = info;
  const gpuUnavailable = settings.gpu !== 'auto' && !graphicsCards.some((card) => card.id === settings.gpu);
  const gpuOptions = useMemo<Option[]>(
    () => [
      { text: t('gpuAuto'), value: 'auto' },
      ...graphicsCards.map((card) => ({
        text:
          platform === 'windows'
            ? card.id === 'ahorro' ? t('gpuPowerSaver') : t('gpuPerformance')
            : `${card.manufacturer} ${card.displayName}`,
        value: card.id,
        detail: (card.kind ? t(card.kind) : '') + (platform === 'linux' ? `  ·  ${card.id}` : ''),
      })),
    ],
    [graphicsCards, platform, t],
  );
  return (
    <div className="flex flex-col gap-1">
      <GroupTitle>{t('groupRenderer')}</GroupTitle>
      <SettingRow icon={MemoryStick} title={t('gpu')} description={gpuUnavailable ? t('gpuMissing') : t('gpuDesc')}>
        <Select label={t('gpu')} options={gpuOptions} value={settings.gpu} onChange={(gpu) => patch({ gpu })} />
      </SettingRow>
      <GroupTitle>{t('groupFrameRate')}</GroupTitle>
      <SettingRow icon={Gauge} title={t('fpsLimit')} description={t('fpsLimitDesc')}>
        <Select
          label={t('fpsLimit')}
          options={[{ text: t('fps60'), value: 60 }, { text: t('fps120'), value: 120 }, { text: t('fps144'), value: 144 }]}
          value={settings.fpsLimit}
          onChange={(fpsLimit) => patch({ fpsLimit })}
        />
      </SettingRow>
      <SettingRow icon={RefreshCw} title={t('vsync')} description={t('vsyncDesc')}>
        <Switch label={t('vsync')} checked={settings.vsync} onChange={(vsync) => patch({ vsync })} />
      </SettingRow>
      <SettingRow icon={Activity} title={t('showFps')} description={t('showFpsDesc')}>
        <Switch label={t('showFps')} checked={settings.showFps} onChange={(showFps) => patch({ showFps })} />
      </SettingRow>
      {optimizing && <GroupTitle>{t('groupOptimization')}</GroupTitle>}
      <SettingRow hidden={!optimizing} icon={Cpu} title={t('optimizationEnabled')} description={t('optimizationEnabledDesc')}>
        <Switch label={t('optimizationEnabled')} checked={settings.optimizationEnabled} onChange={(optimizationEnabled) => patch({ optimizationEnabled })} />
      </SettingRow>
      <SettingRow hidden={!optimizing} disabled={!settings.optimizationEnabled} icon={Hourglass} title={t('optimizeDuringGame')} description={t('optimizeDuringGameDesc')}>
        <Switch label={t('optimizeDuringGame')} checked={settings.optimizeDuringGame} onChange={(optimizeDuringGame) => patch({ optimizeDuringGame })} />
      </SettingRow>
      {optimizing && settings.optimizationEnabled && settings.optimizeDuringGame && (
        <Note tone="warn" title={t('optimizeDuringGameWarningTitle')}>
          {t('optimizeDuringGameWarning')}
        </Note>
      )}
      <SettingRow
        hidden={!(optimizing && optimizer.configured)}
        icon={Rocket} title={t('optimizeNow')}
        description={f('optimizeNowDesc', optimizer.compiled, optimizer.captured)}
      >
        <SecondaryButton disabled={optimizer.status === 'compiling'} onClick={() => void api.optimizeNow()}>
          {t('optimizeNowButton')}
        </SecondaryButton>
      </SettingRow>
      <div className="mt-[18px]">
        <Note title={t('noteFpsTitle')}>{t('noteFps')}</Note>
      </div>
    </div>
  );
}
