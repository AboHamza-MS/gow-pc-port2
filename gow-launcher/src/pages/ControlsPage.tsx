import { useEffect, useMemo, useRef, useState } from 'react';
import { Circle, DoorOpen, Ellipsis, Gamepad2, Square, Swords, Triangle, X } from 'lucide-react';
import { Glyphs, GroupTitle, KeyCap, Note, SecondaryButton, Select, SettingRow, Switch } from '../components/ui';
import { Modal } from '../components/Shell';
import { upper } from '../lib/i18n';
import { actions, assign, conflicts, fromEvent, fromMouse, groups, isReserved, keyName, readBindings, type Bindings } from '../lib/keys';
import { assignTarget, parsePadMap, presetLayout, serializePadMap, sharedTargets, SOURCES, TARGETS, type Source, type Target } from '../lib/padmap';
import { useApp } from '../state/app';

type Face = 'cross' | 'circle' | 'square' | 'triangle';

/** Four custom face buttons, or the classic / modern preset. */
export function faceButtons(padButtons: string, modern: boolean): Face[] {
  const custom = padButtons.split(',');
  if (custom.length === 4) return custom as Face[];
  return modern ? ['cross', 'triangle', 'circle', 'square'] : ['cross', 'circle', 'square', 'triangle'];
}

/** Sets one face button; the button that held `value` takes the previous one (a swap). */
export function setFaceButton(current: Face[], index: number, value: Face): Face[] {
  const next = [...current];
  const previous = next[index];
  next.forEach((button, i) => {
    if (i !== index && button === value) next[i] = previous;
  });
  next[index] = value;
  return next;
}

function KeySlot({
  code, capturing, warning, onSelect, onClear,
}: { code: number; capturing: boolean; warning: string; onSelect: () => void; onClear: () => void }) {
  const { t } = useApp();
  const conflict = warning.length > 0;
  return (
    <button
      type="button"
      title={warning || undefined}
      onClick={onSelect}
      onContextMenu={(event) => {
        event.preventDefault();
        if (code !== 0) onClear();
      }}
      className={`relative h-8 w-[108px] rounded-[5px] border pb-px text-[13px] font-semibold transition-colors ${
        capturing
          ? 'border-2 border-accent bg-[#f3e4c6] text-accent'
          : `bg-[#e9d8b8] hover:bg-[#f3e4c6] ${conflict ? 'border-warn' : 'border-line hover:border-line-light'}`
      } ${capturing ? '' : code !== 0 ? (conflict ? 'text-warn' : 'text-ink') : 'text-ink-muted'}`}
    >
      {capturing ? <Ellipsis size={18} className="mx-auto" /> : <Glyphs>{keyName(code, t)}</Glyphs>}
      {code !== 0 && <span className="absolute inset-x-px bottom-px h-[3px] rounded-[3px] bg-[#8a6a42]/60" />}
      {conflict && <span className="absolute right-1 top-1 h-[7px] w-[7px] rounded-full bg-warn" />}
    </button>
  );
}

function CaptureDialog({ action, onCode, onCancel }: { action: string; onCode: (code: number) => void; onCancel: () => void }) {
  const { t, f, lang } = useApp();
  const handlers = useRef({ onCode, onCancel });
  handlers.current = { onCode, onCancel };
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.repeat) return;
      if (event.key === 'Escape') handlers.current.onCancel();
      else handlers.current.onCode(fromEvent(event));
    };
    const mousedown = (event: MouseEvent) => {
      event.preventDefault();
      handlers.current.onCode(fromMouse(event.button));
    };
    const contextmenu = (event: Event) => event.preventDefault();
    // Capture phase: nothing else sees the keys while the dialog is open.
    window.addEventListener('keydown', keydown, true);
    window.addEventListener('mousedown', mousedown, true);
    window.addEventListener('contextmenu', contextmenu, true);
    return () => {
      window.removeEventListener('keydown', keydown, true);
      window.removeEventListener('mousedown', mousedown, true);
      window.removeEventListener('contextmenu', contextmenu, true);
    };
  }, []);
  return (
    <Modal width={470}>
      <div className="flex flex-col items-center gap-3 py-2 text-center">
        <h2 className="font-title text-[22px] font-bold tracking-[0.1em] text-gold-light">{upper(lang, t('captureTitle'))}</h2>
        <p className="text-sm text-ink-soft">{f('captureDesc', action ? `"${t(`act_${action}`)}"` : '')}</p>
        <div className="h-1 w-16 animate-pulse-soft rounded-sm bg-gold" />
      </div>
    </Modal>
  );
}

const faceIcons = [X, Circle, Square, Triangle];

export function ControlsPage() {
  const { settings, patch, t, f, lang } = useApp();
  const map = useMemo(() => readBindings(settings.keys), [settings.keys]);
  const [capture, setCapture] = useState<{ action: string; index: number } | null>(null);
  const faces = faceButtons(settings.padButtons, settings.dmc5Controls);
  // A custom button map the game accepts; an invalid text is ignored by the game too, so it counts as no map.
  const parsedMap = useMemo(() => parsePadMap(settings.padMap), [settings.padMap]);
  const custom = parsedMap.ok ? parsedMap.layout : null;
  const [noStart, setNoStart] = useState(false);
  const scheme = custom ? 'custom' : settings.dmc5Controls ? 'modern' : 'classic';

  const changeScheme = (value: string) => {
    setNoStart(false);
    if (value === 'custom') patch({ padMap: serializePadMap(presetLayout(settings.dmc5Controls, faces)) });
    else patch({ dmc5Controls: value === 'modern', padButtons: '', padMap: '' });
  };
  const changeTarget = (source: Source, target: Target) => {
    if (!custom) return;
    const next = assignTarget(custom, source, target);
    setNoStart(next === null);
    if (next) patch({ padMap: serializePadMap(next) });
  };
  const sharedLabels = custom ? sharedTargets(custom).map((target) => t(`padmapTarget_${target}`)) : [];

  const save = (bindings: Bindings) => patch({ keys: JSON.stringify(bindings) });
  const warning = (action: string, code: number) => {
    if (!code) return '';
    if (isReserved(code)) return t('reservedNotice');
    const others = conflicts(map, action, code);
    return others.length === 0 ? '' : f('conflict', others.map((id) => t(`act_${id}`)).join(', '));
  };
  const faceOptions = [
    { text: t('padCross'), value: 'cross' as Face },
    { text: t('padCircle'), value: 'circle' as Face },
    { text: t('padSquare'), value: 'square' as Face },
    { text: t('padTriangle'), value: 'triangle' as Face },
  ];

  return (
    <div className="flex flex-col gap-1">
      <SettingRow icon={Gamepad2} title={t('padScheme')} description={t('padSchemeDesc')}>
        <Select
          label={t('padScheme')}
          options={[
            { text: t('modernPad'), value: 'modern' },
            { text: t('classicPad'), value: 'classic' },
            { text: t('padmapCustom'), value: 'custom' },
          ]}
          value={scheme}
          onChange={changeScheme}
        />
      </SettingRow>
      {['A / ✕', 'B / ○', 'X / □', 'Y / △'].map((title, index) => (
        <SettingRow
          key={title}
          icon={faceIcons[index]}
          title={title}
          description={custom ? t('padmapLocked') : t('padButtonDesc')}
          disabled={custom !== null}
        >
          <Select
            label={title}
            disabled={custom !== null}
            options={faceOptions}
            value={faces[index]}
            onChange={(value) => patch({ padButtons: setFaceButton(faces, index, value).join(',') })}
          />
        </SettingRow>
      ))}
      {custom && (
        <>
          <GroupTitle>{t('padmapTitle')}</GroupTitle>
          <p className="px-1 text-[13px] text-ink-muted">{t('padmapDesc')}</p>
          {SOURCES.map((source) => (
            <SettingRow key={source} title={t(`padmapSrc_${source}`)}>
              <Select
                label={f('padmapPresses', t(`padmapSrc_${source}`))}
                options={TARGETS.map((target) => ({ text: t(`padmapTarget_${target}`), value: target }))}
                value={custom[source]}
                onChange={(target) => changeTarget(source, target)}
              />
            </SettingRow>
          ))}
          {sharedLabels.length > 0 && (
            <Note tone="warn" title={t('padmapSharedTitle')}>
              {f('padmapShared', sharedLabels.join(', '))}
            </Note>
          )}
          {noStart && <Note tone="warn">{t('padmapNoStart')}</Note>}
          <div className="flex justify-end py-1">
            <SecondaryButton
              onClick={() => {
                setNoStart(false);
                patch({ padMap: '' });
              }}
            >
              {t('padmapReset')}
            </SecondaryButton>
          </div>
        </>
      )}
      <SettingRow icon={Swords} title={t('cancelAnimations')} description={t('cancelAnimationsDesc')}>
        <Switch label={t('cancelAnimations')} checked={settings.cancelAnimations} onChange={(cancelAnimations) => patch({ cancelAnimations })} />
      </SettingRow>
      <SettingRow icon={DoorOpen} title={t('escQuits')} description={t('escQuitsDesc')}>
        <Switch label={t('escQuits')} checked={settings.escQuits} onChange={(escQuits) => patch({ escQuits })} />
      </SettingRow>

      <div className="mt-[22px] flex items-center gap-3">
        <h2 className="flex-1 font-title text-[17px] font-bold tracking-[0.12em] text-gold-light">{upper(lang, t('keyboardTitle'))}</h2>
        <SecondaryButton onClick={() => patch({ keys: '' })}>{t('keyboardReset')}</SecondaryButton>
      </div>
      <p className="mt-1 text-[13px] text-ink-muted">{t('keyboardDesc')}</p>

      {groups.map((group) => (
        <section key={group} className="mt-5 flex flex-col gap-0.5">
          <GroupTitle>{t(group)}</GroupTitle>
          {actions
            .filter((action) => action.group === group)
            .map((action) => {
              const codes = map[action.id] ?? [];
              return (
                <div key={action.id} className="flex h-[42px] items-center gap-3 rounded-[5px] pl-2 pr-1 hover:bg-[#6f4a22]/12">
                  <span className="flex h-6 w-[58px] shrink-0 items-center justify-center rounded-xl border border-line bg-field font-semibold text-ink-soft" style={{ fontSize: action.button.length > 4 ? 10 : 12 }}>
                    <Glyphs size={action.button.length > 4 ? 11 : 13}>{action.button}</Glyphs>
                  </span>
                  <span className="min-w-0 flex-1 truncate text-sm text-ink">{t(`act_${action.id}`)}</span>
                  {[0, 1].map((index) => {
                    const code = codes[index] ?? 0;
                    return (
                      <KeySlot
                        key={index}
                        code={code}
                        capturing={capture?.action === action.id && capture.index === index}
                        warning={warning(action.id, code)}
                        onSelect={() => setCapture({ action: action.id, index: Math.min(index, codes.length) })}
                        onClear={() => save(assign(map, action.id, index, 0))}
                      />
                    );
                  })}
                </div>
              );
            })}
        </section>
      ))}

      <GroupTitle>{t('systemKeys')}</GroupTitle>
      <div className="mb-3 ml-2 grid grid-cols-[auto_1fr_auto_1fr] items-center gap-x-3 gap-y-2">
        <KeyCap>F11</KeyCap>
        <span className="text-sm text-ink-soft">{t('tScreen')}</span>
        <KeyCap>F1</KeyCap>
        <span className="text-sm text-ink-soft">{t('tPanel')}</span>
        <KeyCap>F2</KeyCap>
        <span className="text-sm text-ink-soft">{t('tCheats')}</span>
      </div>

      {capture && (
        <CaptureDialog
          action={capture.action}
          onCancel={() => setCapture(null)}
          onCode={(code) => {
            if (code > 0) save(assign(map, capture.action, capture.index, code));
            setCapture(null);
          }}
        />
      )}
    </div>
  );
}
