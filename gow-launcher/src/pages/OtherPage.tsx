import { Download, FileText, Info, Languages, Link, Minimize2, RotateCcw, SkipForward, Zap } from 'lucide-react';
import { SecondaryButton, Select, SettingRow, Switch } from '../components/ui';
import { useApp } from '../state/app';

export function OtherPage() {
  const { settings, patch, reset, api, info, notify, t } = useApp();
  const shortcut = async (desktop: boolean) => {
    if (await api.createShortcut(desktop)) notify(t('shortcutDone'));
  };

  return (
    <div className="flex flex-col gap-1">
      <SettingRow icon={Languages} title={t('language')}>
        <Select
          label={t('language')}
          options={[{ text: 'Português (Brasil)', value: 'pt-BR' }, { text: 'English', value: 'en' }]}
          value={settings.language}
          onChange={(language) => patch({ language })}
        />
      </SettingRow>
      <SettingRow icon={SkipForward} title={t('skipVideos')} description={t('skipVideosDesc')}>
        <Switch label={t('skipVideos')} checked={settings.skipVideos} onChange={(skipVideos) => patch({ skipVideos })} />
      </SettingRow>
      <SettingRow icon={Zap} title={t('fastBoot')} description={t('fastBootDesc')}>
        <Switch label={t('fastBoot')} checked={settings.fastBoot} onChange={(fastBoot) => patch({ fastBoot })} />
      </SettingRow>
      <SettingRow icon={Minimize2} title={t('minimizeOnPlay')} description={t('minimizeOnPlayDesc')}>
        <Switch label={t('minimizeOnPlay')} checked={settings.minimizeOnPlay} onChange={(minimizeOnPlay) => patch({ minimizeOnPlay })} />
      </SettingRow>
      <SettingRow icon={Download} title={t('autoUpdate')} description={t('autoUpdateDesc')}>
        <Switch label={t('autoUpdate')} checked={settings.autoUpdate} onChange={(autoUpdate) => patch({ autoUpdate })} />
      </SettingRow>
      <SettingRow icon={Link} title={t('shortcuts')} description={t('shortcutsDesc')}>
        <div className="flex gap-2">
          <SecondaryButton onClick={() => void shortcut(false)}>{t('menuShortcut')}</SecondaryButton>
          <SecondaryButton onClick={() => void shortcut(true)}>{t('desktopShortcut')}</SecondaryButton>
        </div>
      </SettingRow>
      <SettingRow icon={FileText} title={t('log')} description={t('logDesc')}>
        <SecondaryButton onClick={() => void api.openLog()}>{t('open')}</SecondaryButton>
      </SettingRow>
      <SettingRow icon={RotateCcw} title={t('reset')} description={t('resetDesc')}>
        <SecondaryButton danger onClick={() => void reset()}>{t('reset')}</SecondaryButton>
      </SettingRow>
      <SettingRow icon={Info} title={t('about')} description={t('aboutText')}>
        <span className="text-[13px] text-ink-muted">v{info.version}</span>
      </SettingRow>
    </div>
  );
}
