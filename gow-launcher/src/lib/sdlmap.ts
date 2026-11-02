// A hand-made SDL mapping for a controller the system has no button layout for (`Settings.padSdlMap`): the interface's copy of the
// rules in `gow-core/src/controller.rs`, tested against the same `sdlmap-cases.json`. The raw numbers are GLFW's, as the game
// reports them (`pads/live.json`): the web page's own gamepad API orders buttons and axes differently, so it is never used.
import type { LiveDevice } from './types';

export const SDL_TARGETS = [
  'a', 'b', 'x', 'y', 'back', 'guide', 'start', 'leftstick', 'rightstick', 'leftshoulder', 'rightshoulder', 'dpup', 'dpdown',
  'dpleft', 'dpright', 'leftx', 'lefty', 'rightx', 'righty', 'lefttrigger', 'righttrigger',
] as const;
export type SdlTarget = (typeof SDL_TARGETS)[number];

/** What the dialog asks for: every target but the guide button, which the game does not read. */
export const ASKED_TARGETS: readonly SdlTarget[] = SDL_TARGETS.filter((target) => target !== 'guide');

export const STICK_AXES: readonly SdlTarget[] = ['leftx', 'lefty', 'rightx', 'righty'];
export const TRIGGER_AXES: readonly SdlTarget[] = ['lefttrigger', 'righttrigger'];

export type Raw =
  | { kind: 'button'; index: number }
  | { kind: 'hat'; index: number; mask: number }
  | { kind: 'axis'; index: number; half: 'whole' | '+' | '-'; invert: boolean };

export type SdlMapping = Partial<Record<SdlTarget, Raw>>;

const MAX_BUTTON = 255;
const MAX_HAT = 31;
const MAX_AXIS = 63;

export function formatRaw(raw: Raw): string {
  if (raw.kind === 'button') return `b${raw.index}`;
  if (raw.kind === 'hat') return `h${raw.index}.${raw.mask}`;
  return `${raw.half === 'whole' ? '' : raw.half}a${raw.index}${raw.invert ? '~' : ''}`;
}

function parseIndex(text: string, max: number): number | null {
  if (!/^[0-9]+$/.test(text)) return null;
  const value = Number(text);
  return value <= max ? value : null;
}

export function parseRaw(text: string): Raw | null {
  const sign = text[0] === '+' || text[0] === '-' ? text[0] : '';
  const rest = text.slice(sign.length);
  const half = sign === '+' ? '+' : sign === '-' ? '-' : 'whole';
  if (rest[0] === 'a') {
    const invert = rest.endsWith('~');
    const index = parseIndex(rest.slice(1, invert ? -1 : undefined), MAX_AXIS);
    return index === null ? null : { kind: 'axis', index, half, invert };
  }
  if (sign !== '') return null;
  if (rest[0] === 'b') {
    const index = parseIndex(rest.slice(1), MAX_BUTTON);
    return index === null ? null : { kind: 'button', index };
  }
  if (rest[0] === 'h') {
    const body = rest.slice(1);
    const dot = body.indexOf('.');
    if (dot < 0) return null;
    const hat = parseIndex(body.slice(0, dot), MAX_HAT);
    const bits = parseIndex(body.slice(dot + 1), 8);
    return hat === null || bits === null || ![1, 2, 4, 8].includes(bits) ? null : { kind: 'hat', index: hat, mask: bits };
  }
  return null;
}

export type ParseResult = { ok: true; mapping: SdlMapping } | { ok: false; error: string };

/** The fields of an SDL mapping (`a:b0,start:b7,...`): each target once, `start` required. */
export function parseFields(fields: string): ParseResult {
  if (fields.trim() === '') return { ok: false, error: 'empty' };
  const mapping: SdlMapping = {};
  const entries = (fields.endsWith(',') ? fields.slice(0, -1) : fields).split(',');
  for (const entry of entries) {
    const colon = entry.indexOf(':');
    if (colon < 0) return { ok: false, error: `'${entry}' is not target:source` };
    const target = entry.slice(0, colon);
    const source = entry.slice(colon + 1);
    if (!(SDL_TARGETS as readonly string[]).includes(target)) return { ok: false, error: `unknown target '${target}'` };
    if ((mapping as Record<string, Raw>)[target]) return { ok: false, error: `target '${target}' appears twice` };
    const raw = parseRaw(source);
    if (!raw) return { ok: false, error: `'${source}' is not a valid input for '${target}'` };
    (mapping as Record<string, Raw>)[target] = raw;
  }
  if (!mapping.start) return { ok: false, error: 'nothing is mapped to start' };
  return { ok: true, mapping };
}

export const fieldsAreValid = (fields: string) => parseFields(fields).ok;

/** The text the settings keep: every mapped target, in a fixed order. */
export function serializeFields(mapping: SdlMapping): string {
  return SDL_TARGETS.flatMap((target) => {
    const raw = mapping[target];
    return raw ? [`${target}:${formatRaw(raw)}`] : [];
  }).join(',');
}

/** Whether a mapping can be saved: something presses start. */
export const canSave = (mapping: SdlMapping) => mapping.start !== undefined;

/** Whether every input of the mapping exists on a joystick with these counts (what the game listed for it). */
export function fitsDevice(mapping: SdlMapping, device: { buttons: number; hats: number; axes: number }): boolean {
  return Object.values(mapping).every((raw) => {
    if (!raw) return true;
    if (raw.kind === 'button') return raw.index < device.buttons;
    if (raw.kind === 'hat') return raw.index < device.hats;
    return raw.index < device.axes;
  });
}

/** Targets that share one raw input with another target (allowed, but worth a warning). */
export function sharedInputs(mapping: SdlMapping): SdlTarget[] {
  const seen = new Map<string, SdlTarget[]>();
  for (const target of SDL_TARGETS) {
    const raw = mapping[target];
    if (!raw) continue;
    const key = formatRaw(raw).replace('~', '');
    seen.set(key, [...(seen.get(key) ?? []), target]);
  }
  return [...seen.values()].filter((targets) => targets.length > 1).flat();
}

// ------------------------------------------------------------------------------------------------ listening to the game

/** A raw input that just started: what the player pressed or moved. `delta` is the axis distance from rest, in hundredths. */
export type LiveInput =
  | { kind: 'button'; index: number }
  | { kind: 'hat'; index: number; bits: number }
  | { kind: 'axis'; index: number; delta: number };

/** The first input that is in `now` and was not in `before`: buttons first, then hats, then axes. */
export function newInput(before: LiveDevice | null, now: LiveDevice): LiveInput | null {
  const button = now.buttons.find((index) => !before?.buttons.includes(index));
  if (button !== undefined) return { kind: 'button', index: button };
  const hat = now.hats.find(([index, bits]) => !before?.hats.some(([i, b]) => i === index && b === bits));
  if (hat) return { kind: 'hat', index: hat[0], bits: hat[1] };
  const axis = now.axes.find(([index]) => !before?.axes.some(([i]) => i === index));
  return axis ? { kind: 'axis', index: axis[0], delta: axis[1] } : null;
}

/**
 * What the input means for `target`, or null when it cannot be that: a stick axis wants an axis (the player pushes it right or
 * down, the way SDL counts, so an axis that grows the other way is inverted); a trigger wants an axis (resting low, or inverted)
 * or a button; any other target wants a button, one direction of a hat, or a half axis.
 */
export function classify(target: SdlTarget, input: LiveInput): Raw | null {
  if (STICK_AXES.includes(target)) {
    return input.kind === 'axis' ? { kind: 'axis', index: input.index, half: 'whole', invert: input.delta < 0 } : null;
  }
  if (TRIGGER_AXES.includes(target)) {
    if (input.kind === 'button') return { kind: 'button', index: input.index };
    return input.kind === 'axis' ? { kind: 'axis', index: input.index, half: 'whole', invert: input.delta < 0 } : null;
  }
  if (input.kind === 'button') return { kind: 'button', index: input.index };
  if (input.kind === 'hat') return [1, 2, 4, 8].includes(input.bits) ? { kind: 'hat', index: input.index, mask: input.bits } : null;
  return { kind: 'axis', index: input.index, half: input.delta > 0 ? '+' : '-', invert: false };
}
