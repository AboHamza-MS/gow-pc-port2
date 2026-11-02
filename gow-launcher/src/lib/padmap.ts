// The gamepad button map (`Settings.padMap`, the game's `PC_PAD_MAP`): the interface's copy of the rules in
// `gow-core/src/padmap.rs`. Both are tested against `padmap-cases.json`.

export const SOURCES = ['a', 'b', 'x', 'y', 'lb', 'rb', 'lt', 'rt', 'back', 'start', 'ls', 'rs', 'up', 'down', 'left', 'right'] as const;
export const TARGETS = [
  'cross', 'circle', 'square', 'triangle', 'l1', 'r1', 'l2', 'r2', 'select', 'start', 'l3', 'r3', 'up', 'down', 'left', 'right', 'none',
] as const;

export type Source = (typeof SOURCES)[number];
export type Target = (typeof TARGETS)[number];
/** What every physical button presses. A source that is missing from a text means `none`. */
export type Layout = Record<Source, Target>;

const isSource = (name: string): name is Source => (SOURCES as readonly string[]).includes(name);
const isTarget = (name: string): name is Target => (TARGETS as readonly string[]).includes(name);

export type ParseResult = { ok: true; layout: Layout } | { ok: false; error: string };

/** Reads a `PC_PAD_MAP` text. Same rules as the game: a source once, `start` pressed by some source, nothing else unknown. */
export function parsePadMap(text: string): ParseResult {
  if (text === '') return { ok: false, error: 'empty' };
  const layout = Object.fromEntries(SOURCES.map((source) => [source, 'none'])) as Layout;
  const listed = new Set<Source>();
  const parts = text.split(';');
  if (parts.length > 1 && parts[parts.length - 1].trim() === '') parts.pop(); // one trailing separator is fine
  for (const part of parts) {
    const entry = part.trim();
    if (entry === '') return { ok: false, error: 'empty entry' };
    const equals = entry.indexOf('=');
    if (equals < 0) return { ok: false, error: `'${entry}' is not source=target` };
    const source = entry.slice(0, equals).trim();
    const target = entry.slice(equals + 1).trim();
    if (!isSource(source)) return { ok: false, error: `unknown source '${source}'` };
    if (!isTarget(target)) return { ok: false, error: `unknown target '${target}'` };
    if (listed.has(source)) return { ok: false, error: `source '${source}' appears twice` };
    listed.add(source);
    layout[source] = target;
  }
  if (!hasStart(layout)) return { ok: false, error: 'no button presses start' };
  return { ok: true, layout };
}

export const padMapIsValid = (text: string) => parsePadMap(text).ok;

/** The text the game reads: every source, in the fixed order. */
export function serializePadMap(layout: Layout): string {
  return SOURCES.map((source) => `${source}=${layout[source]}`).join(';');
}

export const hasStart = (layout: Layout) => SOURCES.some((source) => layout[source] === 'start');

/**
 * The layout a preset gives, so the first edit of a custom map starts from what the player has now: the classic or
 * modern shoulders, and `faces` for A, B, X, Y (the face buttons the player swapped, or the preset's own).
 */
export function presetLayout(modern: boolean, faces: readonly string[]): Layout {
  const face = (index: number, fallback: Target): Target => (isTarget(faces[index] ?? '') ? (faces[index] as Target) : fallback);
  return {
    a: face(0, 'cross'),
    b: face(1, modern ? 'triangle' : 'circle'),
    x: face(2, modern ? 'circle' : 'square'),
    y: face(3, modern ? 'square' : 'triangle'),
    lb: 'l1',
    rb: 'r1',
    lt: modern ? 'r2' : 'l2',
    rt: modern ? 'l2' : 'r2',
    back: 'select',
    start: 'start',
    ls: 'l3',
    rs: 'r3',
    up: 'up',
    down: 'down',
    left: 'left',
    right: 'right',
  };
}

/** Targets that more than one button presses (allowed, but worth a warning), `none` excluded. */
export function sharedTargets(layout: Layout): Target[] {
  const counts = new Map<Target, number>();
  for (const source of SOURCES) counts.set(layout[source], (counts.get(layout[source]) ?? 0) + 1);
  return TARGETS.filter((target) => target !== 'none' && (counts.get(target) ?? 0) > 1);
}

/** `layout` with `source` pressing `target`; null when that would leave nothing to start the game with. */
export function assignTarget(layout: Layout, source: Source, target: Target): Layout | null {
  const next = { ...layout, [source]: target };
  return hasStart(next) ? next : null;
}
