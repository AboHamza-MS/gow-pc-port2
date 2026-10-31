import { describe, expect, it } from 'vitest';
import { assign, conflicts, defaultBindings, fromEvent, fromMouse, isReserved, keyName, readBindings } from './keys';

// Ported from launcher/tests/keyboard_test.qml.
describe('keys', () => {
  it('has the default scheme', () => {
    const map = readBindings('');
    expect(map.l2).toEqual([340]);
    expect(map.r2).toEqual([341]);
    expect(map.l1).toEqual([81]);
    expect(map.up).toEqual([265]);
    expect(map.poseidon).toEqual([49]);
    expect(map.medusa).toEqual([50]);
    expect(map.zeus).toEqual([51]);
    expect(map.hades).toEqual([52]);
    expect(map.artemis).toEqual([1002]);
    expect(map.square).toEqual([1000]);
    expect(conflicts(map, 'r2', 341)).toHaveLength(0); // Ctrl as dodge and R2 is intentional
  });

  it('migrates a map saved before the magic actions', () => {
    const old = JSON.stringify({ up: [265, 49], right: [262, 50], down: [264, 51], left: [263, 52], r2: [1004] });
    const map = readBindings(old);
    expect(map.up).toEqual([265]);
    expect(map.left).toEqual([263]);
    expect(map.poseidon).toEqual([49]);
    expect(map.hades).toEqual([52]);
    expect(map.r2).toEqual([1004]);
    expect(conflicts(map, 'poseidon', 49)).toHaveLength(0);
  });

  it('keeps the d-pad digits of a current map', () => {
    expect(readBindings('{"poseidon":[90],"up":[265,49]}').up).toEqual([265, 49]);
  });

  it('survives garbage', () => {
    expect(readBindings('not json')).toEqual(defaultBindings());
    expect(readBindings('[1,2]')).toEqual(defaultBindings());
  });

  it('assigns a second key and moves a duplicate', () => {
    const map = assign(readBindings(''), 'l2', 1, 88);
    expect(map.l2).toEqual([340, 88]);
    const moved = assign(map, 'l2', 0, 88); // 88 already sits in slot 1
    expect(moved.l2).toEqual([88]);
  });

  it('reports conflicts and clears with a non-positive code', () => {
    let map = assign(readBindings(''), 'l1', 0, 87); // Q -> W, which is already "forward"
    expect(conflicts(map, 'l1', 87)).toEqual(['ls_up']);
    map = assign(map, 'l1', 0, 0);
    expect(map.l1).toEqual([]);
  });

  it('treats the left and right modifier as the same key', () => {
    const map = assign(readBindings(''), 'cross', 0, 344); // right Shift == Shift (L2)
    expect(conflicts(map, 'cross', 344)).toContain('l2');
  });

  it('maps physical keys and mouse buttons', () => {
    expect(fromEvent({ code: 'KeyW' })).toBe(87);
    expect(fromEvent({ code: 'Digit4' })).toBe(52);
    expect(fromEvent({ code: 'Numpad0' })).toBe(320);
    expect(fromEvent({ code: 'F11' })).toBe(300);
    expect(fromEvent({ code: 'ControlRight' })).toBe(345);
    expect(fromEvent({ code: 'Space' })).toBe(32);
    expect(fromEvent({ code: 'Unidentified' })).toBe(0);
    expect(fromMouse(0)).toBe(1000);
    expect(fromMouse(2)).toBe(1001);
    expect(fromMouse(1)).toBe(1002);
    expect(fromMouse(9)).toBe(0);
  });

  it('names keys', () => {
    const t = (key: string) => key;
    expect(keyName(0, t)).toBe('—');
    expect(keyName(87, t)).toBe('W');
    expect(keyName(299, t)).toBe('F10');
    expect(keyName(1002, t)).toBe('mouseMiddle');
    expect(keyName(32, t)).toBe('space');
    expect(keyName(999, t)).toBe('#999');
    expect(isReserved(291)).toBe(true);
    expect(isReserved(65)).toBe(false);
  });
});
