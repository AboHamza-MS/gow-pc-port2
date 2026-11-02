import { describe, expect, it } from 'vitest';
import cases from './sdlmap-cases.json';
import {
  canSave, classify, fieldsAreValid, fitsDevice, formatRaw, newInput, parseFields, parseRaw, serializeFields, sharedInputs,
  type SdlMapping,
} from './sdlmap';
import type { LiveDevice } from './types';

const live = (extra: Partial<LiveDevice> = {}): LiveDevice => ({ index: 3, guid: '0300dddd5e040000a102000000010000', buttons: [], hats: [], axes: [], ...extra });

describe('SDL mapping fields', () => {
  it('agree with the shared table of cases (the Rust side runs the same one)', () => {
    expect(cases.length).toBeGreaterThan(20);
    for (const entry of cases) expect(fieldsAreValid(entry.fields), `${JSON.stringify(entry.fields)} (${entry.note})`).toBe(entry.valid);
  });

  it('write and read raw inputs in SDL spelling', () => {
    const all = ['b0', 'b255', 'h0.4', 'h2.1', 'a2', 'a2~', '+a5', '-a5~'];
    for (const text of all) expect(formatRaw(parseRaw(text)!), text).toBe(text);
    for (const bad of ['', 'b', 'a', 'h0', 'h0.3', 'h0.1.2', 'x1', 'b256', 'a64', 'h32.1', 'b1~', '+b1', '-h0.1', 'a1~~', 'b 1', 'b1.2']) {
      expect(parseRaw(bad), bad).toBeNull();
    }
  });

  it('turn the choices into the fields in a fixed order, and back', () => {
    const mapping: SdlMapping = {
      start: { kind: 'button', index: 7 },
      lefttrigger: { kind: 'axis', index: 2, half: '+', invert: false },
      a: { kind: 'button', index: 0 },
      dpup: { kind: 'hat', index: 0, mask: 1 },
      leftx: { kind: 'axis', index: 0, half: 'whole', invert: false },
    };
    const fields = serializeFields(mapping);
    expect(fields).toBe('a:b0,start:b7,dpup:h0.1,leftx:a0,lefttrigger:+a2');
    const back = parseFields(fields);
    expect(back.ok && serializeFields(back.mapping)).toBe(fields);
    expect(canSave(mapping)).toBe(true);
    expect(canSave({ a: { kind: 'button', index: 0 } })).toBe(false);
    expect(parseFields('a:b0')).toEqual({ ok: false, error: 'nothing is mapped to start' });
    expect(parseFields('a:b0,a:b1,start:b7')).toEqual({ ok: false, error: "target 'a' appears twice" });
  });

  it('check a mapping against the counts the game listed for the device', () => {
    const pad = { buttons: 15, hats: 1, axes: 6 };
    const ok = parseFields('a:b14,start:b7,dpup:h0.1,leftx:a5');
    const tooFar = parseFields('a:b15,start:b7');
    expect(ok.ok && fitsDevice(ok.mapping, pad)).toBe(true);
    expect(tooFar.ok && fitsDevice(tooFar.mapping, pad)).toBe(false);
  });

  it('point out two targets that share one raw input', () => {
    const shared = parseFields('a:b0,b:b0,start:b7,leftx:a0,dpleft:-a0');
    expect(shared.ok && sharedInputs(shared.mapping)).toEqual(['a', 'b']);
    const none = parseFields('a:b0,start:b7');
    expect(none.ok && sharedInputs(none.mapping)).toEqual([]);
  });
});

describe('listening to the game', () => {
  it('sees only an input that was not there before', () => {
    expect(newInput(null, live())).toBeNull();
    expect(newInput(live({ buttons: [2] }), live({ buttons: [2] }))).toBeNull(); // still held from before
    expect(newInput(live({ buttons: [2] }), live({ buttons: [2, 5] }))).toEqual({ kind: 'button', index: 5 });
    expect(newInput(live(), live({ hats: [[0, 4]] }))).toEqual({ kind: 'hat', index: 0, bits: 4 });
    expect(newInput(live({ hats: [[0, 4]] }), live({ hats: [[0, 2]] }))).toEqual({ kind: 'hat', index: 0, bits: 2 });
    expect(newInput(live(), live({ axes: [[2, 200]] }))).toEqual({ kind: 'axis', index: 2, delta: 200 });
    expect(newInput(live({ axes: [[2, 200]] }), live({ axes: [[2, 190]] }))).toBeNull();
    // A button wins over a hat and an axis that move with it.
    expect(newInput(live(), live({ buttons: [1], hats: [[0, 1]], axes: [[0, 100]] }))).toEqual({ kind: 'button', index: 1 });
  });

  it('turns a button, a hat direction or an axis into what the target needs', () => {
    expect(classify('a', { kind: 'button', index: 0 })).toEqual({ kind: 'button', index: 0 });
    expect(classify('dpup', { kind: 'hat', index: 0, bits: 1 })).toEqual({ kind: 'hat', index: 0, mask: 1 });
    expect(classify('dpup', { kind: 'hat', index: 0, bits: 3 })).toBeNull(); // a diagonal is not a direction
    // A d-pad that is two axes: the half the player pressed.
    expect(classify('dpleft', { kind: 'axis', index: 6, delta: -200 })).toEqual({ kind: 'axis', index: 6, half: '-', invert: false });
    expect(classify('dpright', { kind: 'axis', index: 6, delta: 200 })).toEqual({ kind: 'axis', index: 6, half: '+', invert: false });
    // Sticks want axes only, and an axis that grows the wrong way is inverted.
    expect(classify('leftx', { kind: 'axis', index: 0, delta: 150 })).toEqual({ kind: 'axis', index: 0, half: 'whole', invert: false });
    expect(classify('lefty', { kind: 'axis', index: 1, delta: -150 })).toEqual({ kind: 'axis', index: 1, half: 'whole', invert: true });
    expect(classify('rightx', { kind: 'button', index: 3 })).toBeNull();
    expect(classify('righty', { kind: 'hat', index: 0, bits: 1 })).toBeNull();
    // Triggers: an axis (resting low, or inverted) or a button.
    expect(classify('lefttrigger', { kind: 'axis', index: 2, delta: 200 })).toEqual({ kind: 'axis', index: 2, half: 'whole', invert: false });
    expect(classify('righttrigger', { kind: 'axis', index: 5, delta: -200 })).toEqual({ kind: 'axis', index: 5, half: 'whole', invert: true });
    expect(classify('lefttrigger', { kind: 'button', index: 6 })).toEqual({ kind: 'button', index: 6 });
    expect(classify('righttrigger', { kind: 'hat', index: 0, bits: 1 })).toBeNull();
  });

  it('builds the xpad layout of issue #12 from what a pad with 15 buttons, 1 hat and 6 axes reports', () => {
    // The presses a player would make, as the game reports them (GLFW's Linux evdev numbering of the xpad driver).
    const presses: Array<[Parameters<typeof classify>[0], Parameters<typeof newInput>[1]]> = [
      ['a', live({ buttons: [0] })], ['b', live({ buttons: [1] })], ['x', live({ buttons: [2] })], ['y', live({ buttons: [3] })],
      ['leftshoulder', live({ buttons: [4] })], ['rightshoulder', live({ buttons: [5] })], ['back', live({ buttons: [6] })], ['start', live({ buttons: [7] })],
      ['leftstick', live({ buttons: [9] })], ['rightstick', live({ buttons: [10] })],
      ['dpup', live({ hats: [[0, 1]] })], ['dpright', live({ hats: [[0, 2]] })], ['dpdown', live({ hats: [[0, 4]] })], ['dpleft', live({ hats: [[0, 8]] })],
      ['leftx', live({ axes: [[0, 150]] })], ['lefty', live({ axes: [[1, 150]] })], ['rightx', live({ axes: [[3, 150]] })], ['righty', live({ axes: [[4, 150]] })],
      ['lefttrigger', live({ axes: [[2, 200]] })], ['righttrigger', live({ axes: [[5, 200]] })],
    ];
    const mapping: SdlMapping = {};
    for (const [target, device] of presses) {
      const input = newInput(null, device);
      mapping[target] = classify(target, input!) ?? undefined;
    }
    expect(serializeFields(mapping)).toBe(
      'a:b0,b:b1,x:b2,y:b3,back:b6,start:b7,leftstick:b9,rightstick:b10,leftshoulder:b4,rightshoulder:b5,dpup:h0.1,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,' +
        'leftx:a0,lefty:a1,rightx:a3,righty:a4,lefttrigger:a2,righttrigger:a5',
    );
    expect(fieldsAreValid(serializeFields(mapping))).toBe(true);
    expect(fitsDevice(mapping, { buttons: 15, hats: 1, axes: 6 })).toBe(true);
  });
});
