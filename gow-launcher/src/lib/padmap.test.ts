import { describe, expect, it } from 'vitest';
import cases from './padmap-cases.json';
import { assignTarget, hasStart, padMapIsValid, parsePadMap, presetLayout, serializePadMap, sharedTargets, SOURCES } from './padmap';

describe('pad map rules', () => {
  it('agree with the shared table of cases (the Rust side runs the same one)', () => {
    expect(cases.length).toBeGreaterThan(15);
    for (const entry of cases) expect(padMapIsValid(entry.value), `${entry.value} (${entry.note})`).toBe(entry.valid);
  });

  it('say why a value is invalid in the words the game logs', () => {
    expect(parsePadMap('a=cross;b=circle')).toEqual({ ok: false, error: 'no button presses start' });
    expect(parsePadMap('a=cross;a=circle;start=start')).toEqual({ ok: false, error: "source 'a' appears twice" });
    expect(parsePadMap('a=jump')).toEqual({ ok: false, error: "unknown target 'jump'" });
    expect(parsePadMap('guide=a')).toEqual({ ok: false, error: "unknown source 'guide'" });
  });

  it('read a map into a full layout, with the sources it does not list on none', () => {
    const parsed = parsePadMap('a=circle;start=start');
    expect(parsed.ok && parsed.layout.a).toBe('circle');
    expect(parsed.ok && parsed.layout.b).toBe('none');
  });
});

describe('preset layouts', () => {
  it('seed a custom map from the layout the player has now', () => {
    const classic = presetLayout(false, ['cross', 'circle', 'square', 'triangle']);
    expect(serializePadMap(classic)).toBe(
      'a=cross;b=circle;x=square;y=triangle;lb=l1;rb=r1;lt=l2;rt=r2;back=select;start=start;ls=l3;rs=r3;up=up;down=down;left=left;right=right',
    );
    const modern = presetLayout(true, ['cross', 'triangle', 'circle', 'square']);
    expect(modern.lt).toBe('r2');
    expect(modern.rt).toBe('l2');
    expect(modern.b).toBe('triangle');
    // Swapped face buttons are kept, the shoulders still follow the preset.
    const swapped = presetLayout(false, ['circle', 'cross', 'square', 'triangle']);
    expect([swapped.a, swapped.b, swapped.lt]).toEqual(['circle', 'cross', 'l2']);
    // Whatever the layout, the written text is valid and round trips.
    for (const layout of [classic, modern, swapped]) {
      const text = serializePadMap(layout);
      expect(padMapIsValid(text)).toBe(true);
      const back = parsePadMap(text);
      expect(back.ok && back.layout).toEqual(layout);
    }
  });

  it('fall back to the preset for a face value that is not a PS2 button', () => {
    expect(presetLayout(true, ['nope', '', 'x', 'y']).a).toBe('cross');
    expect(presetLayout(true, []).y).toBe('square');
  });
});

describe('editing a layout', () => {
  const layout = presetLayout(false, ['cross', 'circle', 'square', 'triangle']);

  it('warns about shared targets without refusing them', () => {
    expect(sharedTargets(layout)).toEqual([]);
    const shared = assignTarget(layout, 'b', 'cross');
    expect(shared).not.toBeNull();
    expect(sharedTargets(shared!)).toEqual(['cross']);
    // `none` on several buttons is not a share.
    const dropped = assignTarget(assignTarget(layout, 'a', 'none')!, 'b', 'none')!;
    expect(sharedTargets(dropped)).toEqual([]);
  });

  it('refuses the change that leaves nothing pressing start', () => {
    expect(assignTarget(layout, 'start', 'cross')).toBeNull();
    const second = assignTarget(layout, 'back', 'start')!;
    expect(hasStart(second)).toBe(true);
    expect(assignTarget(second, 'start', 'none')).not.toBeNull(); // back still starts
    expect(assignTarget(second, 'back', 'select')).not.toBeNull(); // start still starts
  });

  it('covers all sixteen buttons', () => {
    expect(SOURCES).toHaveLength(16);
    expect(Object.keys(layout)).toHaveLength(16);
  });
});
