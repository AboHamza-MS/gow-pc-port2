import { useEffect, useRef, useState } from 'react';
import { useApp } from '../state/app';
import {
  ASKED_TARGETS, canSave, classify, fitsDevice, newInput, parseFields, serializeFields, sharedInputs, type Raw, type SdlMapping, type SdlTarget,
} from '../lib/sdlmap';
import type { LiveDevice, LiveState, PadDevice } from '../lib/types';
import { Modal } from './Shell';
import { Note, SecondaryButton } from './ui';

/** A game that wrote its raw input less than this long ago is running and listening. */
const FRESH_MS = 3000;

/**
 * The dialog that maps the buttons of a controller the system has no layout for. The raw numbers come from the game itself
 * (`api.padLive`), not from the web page's gamepad API: that one orders buttons and axes its own way, and an SDL mapping is written
 * in the numbering the game (GLFW) uses.
 */
export function PadMappingDialog({ device, initial, onSave, onClose }: {
  device: PadDevice;
  initial: string;
  onSave: (fields: string) => void;
  onClose: () => void;
}) {
  const { api, t, f } = useApp();
  const [mapping, setMapping] = useState<SdlMapping>(() => {
    const parsed = parseFields(initial);
    return parsed.ok ? parsed.mapping : {};
  });
  const [listening, setListening] = useState<SdlTarget | null>(null);
  const [live, setLive] = useState<LiveState | null>(null);
  const baseline = useRef<LiveDevice | null>(null);

  // Ask the game for the live feed while the dialog is open (it stops writing when the request goes stale), and read it.
  useEffect(() => {
    void api.padProbe(true);
    const keepAsking = setInterval(() => void api.padProbe(true), 3000);
    const poll = setInterval(() => void api.padLive().then(setLive, () => setLive(null)), 120);
    return () => {
      clearInterval(keepAsking);
      clearInterval(poll);
      void api.padProbe(false);
    };
  }, [api]);

  const fresh = live !== null && live.ageMs < FRESH_MS;
  const current = fresh ? live.devices.find((candidate) => candidate.guid.toLowerCase() === device.guid.toLowerCase()) ?? null : null;

  // Listening: the first input that was not there when the player pressed Listen becomes the target's input.
  useEffect(() => {
    if (!listening || !current) return;
    const input = newInput(baseline.current, current);
    baseline.current = current;
    if (!input) return;
    const raw = classify(listening, input);
    if (!raw) return; // a button for a stick, say: keep listening
    setMapping((previous) => ({ ...previous, [listening]: raw }));
    setListening(null);
  }, [listening, current]);

  useEffect(() => {
    if (!listening) return;
    const keydown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopPropagation();
      setListening(null);
    };
    window.addEventListener('keydown', keydown, true);
    return () => window.removeEventListener('keydown', keydown, true);
  }, [listening]);

  const rawText = (raw: Raw) => {
    if (raw.kind === 'button') return f('padsdlRawButton', raw.index);
    if (raw.kind === 'hat') return f('padsdlRawHat', raw.index, t(`padsdlDir_${raw.mask}`));
    if (raw.half === '+') return f('padsdlRawAxisPlus', raw.index);
    if (raw.half === '-') return f('padsdlRawAxisMinus', raw.index);
    return f(raw.invert ? 'padsdlRawAxisInverted' : 'padsdlRawAxis', raw.index);
  };
  const startListening = (target: SdlTarget) => {
    baseline.current = current; // what is held now does not count
    setListening(target);
  };
  const clear = (target: SdlTarget) =>
    setMapping((previous) => {
      const next = { ...previous };
      delete next[target];
      return next;
    });
  const shared = sharedInputs(mapping);
  const savable = canSave(mapping) && fitsDevice(mapping, device);

  return (
    <Modal width={680}>
      <div className="flex flex-col gap-3">
        <h2 className="font-title text-[20px] font-bold tracking-[0.06em] text-gold-light">{f('padsdlTitle', device.name)}</h2>
        <p className="text-[13px] leading-[1.25] text-ink-soft">{t('padsdlIntro')}</p>
        {fresh && current && <p className="text-[13px] font-semibold text-ink-soft">{t('padsdlLive')}</p>}
        {!fresh && <Note tone="warn">{t('padsdlWaiting')}</Note>}
        {fresh && !current && <Note tone="warn">{t('padsdlMissing')}</Note>}
        <div className="flex max-h-[44vh] flex-col gap-0.5 overflow-y-auto pr-1">
          {ASKED_TARGETS.map((target) => {
            const raw = mapping[target];
            const active = listening === target;
            return (
              <div key={target} className={`flex min-h-[38px] items-center gap-3 rounded-[5px] pl-2 pr-1 ${active ? 'bg-[#f3e4c6]' : 'hover:bg-[#6f4a22]/12'}`}>
                <span className="min-w-0 flex-1 truncate text-sm text-ink">{t(`padsdlTarget_${target}`)}</span>
                <span className={`w-[170px] shrink-0 truncate text-[13px] font-semibold ${raw ? 'text-ink' : 'text-ink-muted'}`}>
                  {active ? '…' : raw ? rawText(raw) : t('padsdlNotMapped')}
                </span>
                <SecondaryButton
                  className="!h-8 shrink-0"
                  disabled={!current}
                  aria-label={`${t('padsdlListen')} ${t(`padsdlTarget_${target}`)}`}
                  onClick={() => (active ? setListening(null) : startListening(target))}
                >
                  {active ? t('padsdlCancel') : t('padsdlListen')}
                </SecondaryButton>
                <SecondaryButton
                  className="!h-8 shrink-0"
                  disabled={!raw}
                  aria-label={`${t('padsdlClear')} ${t(`padsdlTarget_${target}`)}`}
                  onClick={() => clear(target)}
                >
                  {t('padsdlClear')}
                </SecondaryButton>
              </div>
            );
          })}
        </div>
        {listening && <p className="text-[13px] font-semibold text-accent">{f('padsdlListening', t(`padsdlTarget_${listening}`))}</p>}
        {!canSave(mapping) && <p className="text-[13px] text-ink-muted">{t('padsdlNeedStart')}</p>}
        {shared.length > 0 && <Note tone="warn">{f('padsdlShared', shared.map((target) => t(`padsdlTarget_${target}`)).join(', '))}</Note>}
        <div className="flex justify-between gap-2">
          <div>
            {initial !== '' && (
              <SecondaryButton danger onClick={() => onSave('')}>
                {t('padsdlRemove')}
              </SecondaryButton>
            )}
          </div>
          <div className="flex gap-2">
            <SecondaryButton onClick={onClose}>{t('padsdlCancel')}</SecondaryButton>
            <SecondaryButton disabled={!savable} onClick={() => onSave(serializeFields(mapping))}>
              {t('padsdlSave')}
            </SecondaryButton>
          </div>
        </div>
      </div>
    </Modal>
  );
}
