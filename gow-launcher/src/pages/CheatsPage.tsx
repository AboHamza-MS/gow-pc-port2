import { useEffect, useState } from 'react';
import { BatteryFull, CircleArrowUp, Flame, Gem, Gift, Gamepad2, Shield, Swords, WandSparkles, Zap } from 'lucide-react';
import { Glyphs, KeyCap, SecondaryButton, SettingRow, Switch, type IconComponent } from '../components/ui';
import { useApp } from '../state/app';

/** Same order and identifiers as `kIds` in gow_cheats.cpp. */
export const CHEATS = [
  'god_mode', 'one_hit_kill', 'infinite_magic', 'infinite_rage', 'max_orbs', 'all_upgrades', 'all_powers', 'max_meters',
  'unlock_extras',
];

const CHEAT_ICONS: Record<string, IconComponent> = {
  god_mode: Shield, one_hit_kill: Swords, infinite_magic: WandSparkles, infinite_rage: Flame, max_orbs: Gem,
  all_upgrades: CircleArrowUp, all_powers: Zap, max_meters: BatteryFull, unlock_extras: Gift,
};

const PAD_NAMES: Record<string, string> = {
  '1': 'D-pad ↑', '2': 'D-pad →', '3': 'D-pad ↓', '4': 'D-pad ←', '5': 'Y / △', '6': 'B / ○', '7': 'A / ✕', '8': 'X / □',
  '9': 'LB / L1', '10': 'LT / L2', '11': 'RB / R1', '12': 'RT / R2', '13': 'Back / Select', '15': 'Start',
  '16': 'LS / L3', '17': 'RS / R3',
};

export function bindingName(binding: string, none: string): string {
  if (!binding) return none;
  return binding.split('+').map((code) => PAD_NAMES[code] ?? `#${code}`).join(' + ');
}

/** `active` filtered to the known cheats, in their canonical order, with `id` switched. */
export function toggleCheat(active: string[], id: string, enabled: boolean): string {
  return CHEATS.filter((cheat) => (cheat === id ? enabled : active.includes(cheat))).join(',');
}

export function CheatsPage() {
  const { settings, patch, api, t } = useApp();
  const active = settings.cheats.length > 0 ? settings.cheats.split(',') : [];
  const [capturing, setCapturing] = useState(false);
  const [message, setMessage] = useState('');

  useEffect(() => {
    const stops = [
      api.on('gamepad-captured', ({ binding, label }) => {
        patch({ cheatsShortcut: binding });
        setMessage(label);
        setCapturing(false);
      }),
      api.on('gamepad-failed', ({ reason }) => {
        setMessage(reason === 'unsupported' ? t('cheatsShortcutUnavailable') : t('cheatsShortcutFailed'));
        setCapturing(false);
      }),
    ];
    return () => {
      stops.forEach((stop) => void stop.then((off) => off()));
      void api.gamepadCancel();
    };
  }, [api, patch, t]);

  const begin = async () => {
    setMessage(t('cheatsShortcutCapturing'));
    const started = await api.gamepadBegin();
    setCapturing(started);
    if (!started) setMessage(t('cheatsShortcutUnavailable'));
  };

  return (
    <div className="flex flex-col gap-1">
      <div className="mb-1.5 flex items-center gap-3 rounded-[5px] border border-line bg-field p-3">
        <KeyCap>F2</KeyCap>
        <p className="flex-1 text-[13px] leading-[1.2] text-ink-soft">{t('cheatsNotice')}</p>
        {active.length > 0 && <SecondaryButton onClick={() => patch({ cheats: '' })}>{t('cheatsTurnOff')}</SecondaryButton>}
      </div>

      <SettingRow icon={Gamepad2} title={t('cheatsShortcut')} description={t('cheatsShortcutDesc')} controlWidth={420}>
        <div className="flex w-full items-center gap-2">
          <span className={`min-w-0 flex-1 truncate text-[13px] ${capturing ? 'text-gold-light' : 'text-ink-soft'}`}>
            <Glyphs>{capturing ? message : bindingName(settings.cheatsShortcut, t('cheatsShortcutNone'))}</Glyphs>
          </span>
          <SecondaryButton
            onClick={() => {
              if (capturing) {
                void api.gamepadCancel();
                setCapturing(false);
                setMessage('');
              } else {
                void begin();
              }
            }}
          >
            {capturing ? t('cheatsShortcutCancel') : t('cheatsShortcutCapture')}
          </SecondaryButton>
          {settings.cheatsShortcut.length > 0 && !capturing && (
            <SecondaryButton
              onClick={() => {
                patch({ cheatsShortcut: '' });
                setMessage('');
              }}
            >
              {t('cheatsShortcutClear')}
            </SecondaryButton>
          )}
        </div>
      </SettingRow>
      {!capturing && message && <p className="px-1 text-[13px] text-warn">{message}</p>}

      {settings.retroAchievementsEnabled && settings.retroAchievementsHardcore && (
        <p className="mb-1.5 rounded-[5px] border border-warn bg-field p-3 text-[13px] text-warn">{t('raHardcoreCheatsNotice')}</p>
      )}

      {CHEATS.map((id) => (
        <SettingRow key={id} icon={CHEAT_ICONS[id]} title={t(`cheat_${id}`)} description={t(`cheat_${id}Desc`)}>
          <Switch label={t(`cheat_${id}`)} checked={active.includes(id)} onChange={(enabled) => patch({ cheats: toggleCheat(active, id, enabled) })} />
        </SettingRow>
      ))}
    </div>
  );
}
