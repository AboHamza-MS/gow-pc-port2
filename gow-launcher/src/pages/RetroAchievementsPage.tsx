import { useCallback, useEffect, useState } from 'react';
import { Bell, Disc3, Flame, KeyRound, Trophy, User } from 'lucide-react';
import { SecondaryButton, SettingRow, Switch, TextInput } from '../components/ui';
import { useApp } from '../state/app';

/** Local RetroAchievements settings. The password never touches the settings file: it lives in the credential store. */
export function RetroAchievementsPage() {
  const { settings, patch, status, api, t } = useApp();
  const [stored, setStored] = useState(false);
  const [originalIso, setOriginalIso] = useState(false);
  const [user, setUser] = useState(settings.retroAchievementsUser);
  const [password, setPassword] = useState('');

  const refresh = useCallback(
    async (name: string) => setStored(name.trim().length > 0 && (await api.raHasCredential(name))),
    [api],
  );
  useEffect(() => {
    void refresh(settings.retroAchievementsUser);
  }, [refresh, settings.retroAchievementsUser]);
  useEffect(() => {
    // The ISO chosen under Files only identifies the game if it is the original (RetroAchievements USA hash).
    if (!settings.iso) return setOriginalIso(false);
    let current = true;
    void api.validateIso(settings.iso).then((result) => current && setOriginalIso(result.ok));
    return () => {
      current = false;
    };
  }, [api, settings.iso]);

  // Without an ISO, the hash comes from SCUS_973.99: it only works if it is the game's original ELF.
  const originalElf = !!status?.game.valid && !status.game.modified;
  const enabled = settings.retroAchievementsEnabled;

  const commitUser = () => {
    const name = user.trim();
    setUser(name);
    if (name !== settings.retroAchievementsUser) patch({ retroAchievementsUser: name });
  };
  const savePassword = async () => {
    if (await api.raSaveCredential(settings.retroAchievementsUser, password)) {
      setPassword('');
      await refresh(settings.retroAchievementsUser);
    }
  };
  const removePassword = async () => {
    await api.raClearCredential();
    await refresh(settings.retroAchievementsUser);
  };

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-[11px] rounded-md border border-gold-dark bg-field p-[13px]">
        <Trophy size={20} className="shrink-0 text-gold-light" />
        <p className="text-[13px] leading-[1.2] text-ink-soft">{t('raSummary')}</p>
      </div>

      <SettingRow icon={Trophy} title={t('raEnabled')} description={t('raEnabledDesc')}>
        <Switch label={t('raEnabled')} checked={enabled} onChange={(retroAchievementsEnabled) => patch({ retroAchievementsEnabled })} />
      </SettingRow>
      <SettingRow disabled={!enabled} icon={User} title={t('raUser')} description={t('raUserDesc')}>
        <TextInput
          value={user}
          placeholder={t('raUserPlaceholder')}
          onChange={(event) => setUser(event.target.value)}
          onBlur={commitUser}
          onKeyDown={(event) => event.key === 'Enter' && commitUser()}
          className="h-[38px] w-[290px] text-sm"
        />
      </SettingRow>
      <SettingRow
        disabled={!enabled || settings.retroAchievementsUser.length === 0}
        icon={KeyRound} title={t('raPassword')}
        description={stored ? t('raPasswordSaved') : t('raPasswordDesc')}
        controlWidth={380}
      >
        <div className="flex items-center gap-2">
          <TextInput
            type="password"
            autoComplete="off"
            value={password}
            placeholder={t('raPasswordPlaceholder')}
            onChange={(event) => setPassword(event.target.value)}
            className="h-[38px] w-[190px] text-sm"
          />
          <SecondaryButton disabled={password.length === 0} onClick={() => void savePassword()}>{t('raSave')}</SecondaryButton>
          {stored && <SecondaryButton onClick={() => void removePassword()}>{t('raRemover')}</SecondaryButton>}
        </div>
      </SettingRow>
      <SettingRow disabled={!enabled || !stored} icon={Flame} title={t('raHardcore')} description={t('raHardcoreDesc')}>
        <Switch label={t('raHardcore')} checked={settings.retroAchievementsHardcore} onChange={(retroAchievementsHardcore) => patch({ retroAchievementsHardcore })} />
      </SettingRow>
      <SettingRow disabled={!enabled || !stored} icon={Bell} title={t('raNotifications')} description={t('raNotificationsDesc')}>
        <Switch label={t('raNotifications')} checked={settings.retroAchievementsNotifications} onChange={(retroAchievementsNotifications) => patch({ retroAchievementsNotifications })} />
      </SettingRow>
      <SettingRow
        icon={Disc3} title={t('raIso')}
        description={originalIso ? t('raIsoOk') : originalElf ? t('raIsoElf') : t('raIsoDesc')}
      >
        <span className={`text-[13px] font-semibold ${originalIso || originalElf ? 'text-ok' : 'text-warn'}`}>
          {originalIso ? t('raIsoSource') : originalElf ? t('raIsoRequired') : t('raIsoModified')}
        </span>
      </SettingRow>
    </div>
  );
}
