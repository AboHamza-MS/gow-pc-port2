// Configurable keys: GLFW codes by physical position (as in the game) and 1000 + button for the mouse.
// The map goes to the game in PC_KEYS, serialized on the Rust side (`gow-core::keys`). Everything here is
// for editing the map in the interface.

export const MOUSE_BASE = 1000;

export type Bindings = Record<string, number[]>;

export interface KeyAction {
  id: string;
  group: string;
  /** PS2 pad button the action is sent to. */
  button: string;
}

/** Actions in tab order; `group` groups the rows. */
export const actions: KeyAction[] = [
  { id: 'ls_up', group: 'movementGroup', button: 'L↑' },
  { id: 'ls_down', group: 'movementGroup', button: 'L↓' },
  { id: 'ls_left', group: 'movementGroup', button: 'L←' },
  { id: 'ls_right', group: 'movementGroup', button: 'L→' },
  { id: 'rs_mod', group: 'movementGroup', button: 'R' },
  { id: 'square', group: 'combatGroup', button: '□' },
  { id: 'triangle', group: 'combatGroup', button: '△' },
  { id: 'circle', group: 'combatGroup', button: '○' },
  { id: 'cross', group: 'combatGroup', button: '✕' },
  { id: 'l1', group: 'combatGroup', button: 'L1' },
  { id: 'r1', group: 'combatGroup', button: 'R1' },
  { id: 'artemis', group: 'combatGroup', button: 'L1+R1' },
  { id: 'r2', group: 'powersGroup', button: 'R2' },
  { id: 'l2', group: 'powersGroup', button: 'L2' },
  { id: 'rage', group: 'powersGroup', button: 'L3+R3' },
  // Each magic presses its d-pad direction (the game's controls keyTable, see ps2_pad.cpp).
  { id: 'poseidon', group: 'powersGroup', button: '↓' },
  { id: 'medusa', group: 'powersGroup', button: '←' },
  { id: 'zeus', group: 'powersGroup', button: '→' },
  { id: 'hades', group: 'powersGroup', button: '↑' },
  { id: 'up', group: 'menusGroup', button: '↑' },
  { id: 'right', group: 'menusGroup', button: '→' },
  { id: 'down', group: 'menusGroup', button: '↓' },
  { id: 'left', group: 'menusGroup', button: '←' },
  { id: 'start', group: 'menusGroup', button: 'START' },
  { id: 'select', group: 'menusGroup', button: 'SELECT' },
  { id: 'rs_up', group: 'advancedGroup', button: 'R↑' },
  { id: 'rs_down', group: 'advancedGroup', button: 'R↓' },
  { id: 'rs_left', group: 'advancedGroup', button: 'R←' },
  { id: 'rs_right', group: 'advancedGroup', button: 'R→' },
  { id: 'l3', group: 'advancedGroup', button: 'L3' },
  { id: 'r3', group: 'advancedGroup', button: 'R3' },
];

export const groups = ['movementGroup', 'combatGroup', 'powersGroup', 'menusGroup', 'advancedGroup'];

/** WASD moves, Ctrl + WASD dodges, 1-4 the named magic, clicks attack, middle click draws the Blade of Artemis. */
export function defaultBindings(): Bindings {
  return {
    ls_up: [87], ls_down: [83], ls_left: [65], ls_right: [68],
    rs_mod: [341],
    square: [MOUSE_BASE], triangle: [MOUSE_BASE + 1], circle: [70], cross: [32],
    l1: [81], r1: [69], artemis: [MOUSE_BASE + 2], r2: [341], l2: [340], rage: [86],
    poseidon: [49], medusa: [50], zeus: [51], hades: [52],
    up: [265], right: [262], down: [264], left: [263],
    start: [257], select: [258],
    rs_up: [73], rs_down: [75], rs_left: [74], rs_right: [76],
    l3: [], r3: [],
  };
}

/** Saved map (JSON) with the missing actions taken from the default scheme. Mirrors `gow-core::keys::read_bindings`. */
export function readBindings(text: string): Bindings {
  const base = defaultBindings();
  if (!text) return base;
  try {
    const saved = JSON.parse(text) as Record<string, unknown>;
    if (saved === null || typeof saved !== 'object' || Array.isArray(saved)) return base;
    // Maps from before the per-magic actions: 1-4 were on the d-pad; they now go to the magic's default keys.
    if (!Array.isArray(saved.poseidon)) {
      for (const dpad of ['up', 'right', 'down', 'left']) {
        const items = saved[dpad];
        if (Array.isArray(items)) saved[dpad] = items.filter((c) => typeof c !== 'number' || c < 49 || c > 52);
      }
    }
    for (const { id } of actions) {
      const items = saved[id];
      if (Array.isArray(items)) {
        base[id] = items.slice(0, 2).filter((c): c is number => typeof c === 'number' && c > 0);
      }
    }
  } catch {
    // unreadable: the defaults
  }
  return base;
}

/** Puts `code` in slot `index` of `action` (a code <= 0 clears the slot). A code already on the action moves. */
export function assign(map: Bindings, action: string, index: number, code: number): Bindings {
  const updated: Bindings = Object.fromEntries(Object.entries(map).map(([k, v]) => [k, [...v]]));
  const items = [...(updated[action] ?? [])];
  if (code <= 0) {
    items.splice(index, 1);
  } else {
    const existing = items.indexOf(code);
    if (existing >= 0 && existing !== index) items.splice(existing, 1);
    if (index < items.length) items[index] = code;
    else items.push(code);
  }
  updated[action] = items.slice(0, 2);
  return updated;
}

/** Left and right Shift, Ctrl and Alt are the same key code in the game. */
export function normal(code: number): number {
  return code === 344 ? 340 : code === 345 ? 341 : code === 346 ? 342 : code;
}

/**
 * Other actions that use the same code. Ctrl as dodge and R2 at once is intentional (the game separates the two
 * uses), so that pair does not count as a conflict.
 */
export function conflicts(map: Bindings, action: string, code: number): string[] {
  const result: string[] = [];
  const wanted = normal(code);
  for (const { id: other } of actions) {
    if (other === action) continue;
    for (const item of map[other] ?? []) {
      if (normal(item) !== wanted) continue;
      const paired = (action === 'rs_mod' && other !== 'rs_mod') || (other === 'rs_mod' && action !== 'rs_mod');
      if (!paired) result.push(other);
    }
  }
  return result;
}

/** Keys used by the game or the window itself: F1 (developer panel), F2 (cheats), F11 (fullscreen). */
export function isReserved(code: number): boolean {
  return code === 290 || code === 291 || code === 300;
}

// `KeyboardEvent.code` is the physical position, like GLFW's key codes.
const DOM_CODE_TO_GLFW: Record<string, number> = {
  Escape: 256, Minus: 45, Equal: 61, Backspace: 259, Tab: 258, BracketLeft: 91, BracketRight: 93, Enter: 257,
  ControlLeft: 341, Semicolon: 59, Quote: 39, Backquote: 96, ShiftLeft: 340, Backslash: 92, Comma: 44, Period: 46,
  Slash: 47, ShiftRight: 344, NumpadMultiply: 332, AltLeft: 342, Space: 32, CapsLock: 280, NumLock: 282,
  ScrollLock: 281, NumpadSubtract: 333, NumpadAdd: 334, NumpadDecimal: 330, IntlBackslash: 162, NumpadEnter: 335,
  ControlRight: 345, NumpadDivide: 331, PrintScreen: 283, AltRight: 346, Home: 268, ArrowUp: 265, PageUp: 266,
  ArrowLeft: 263, ArrowRight: 262, End: 269, ArrowDown: 264, PageDown: 267, Insert: 260, Delete: 261, Pause: 284,
  MetaLeft: 343, MetaRight: 347, ContextMenu: 348,
};

/** GLFW key code of a keyboard event, or 0 for a key the game has no code for. */
export function fromEvent(event: Pick<KeyboardEvent, 'code'>): number {
  const { code } = event;
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1].charCodeAt(0);
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return 48 + Number(digit[1]);
  const numpad = /^Numpad([0-9])$/.exec(code);
  if (numpad) return 320 + Number(numpad[1]);
  const fn = /^F([0-9]{1,2})$/.exec(code);
  if (fn) {
    const n = Number(fn[1]);
    if (n >= 1 && n <= 12) return 289 + n;
  }
  return DOM_CODE_TO_GLFW[code] ?? 0;
}

/** DOM `button` (0 left, 1 middle, 2 right, 3 back, 4 forward) -> mouse code (1000 left, 1001 right, 1002 middle). */
export function fromMouse(button: number): number {
  switch (button) {
    case 0: return MOUSE_BASE;
    case 2: return MOUSE_BASE + 1;
    case 1: return MOUSE_BASE + 2;
    case 3: return MOUSE_BASE + 3;
    case 4: return MOUSE_BASE + 4;
    default: return 0;
  }
}

const KEY_NAMES: Record<number, string> = {
  39: "'", 44: ',', 45: '-', 46: '.', 47: '/', 59: ';', 61: '=', 91: '[', 92: '\\', 93: ']', 96: '`', 162: '<>',
  256: 'Esc', 257: 'Enter', 258: 'Tab', 259: '⌫', 260: 'Ins', 261: 'Del', 262: '→', 263: '←', 264: '↓', 265: '↑',
  266: 'PgUp', 267: 'PgDn', 268: 'Home', 269: 'End', 280: 'Caps', 281: 'Scroll', 282: 'NumLk', 283: 'PrtSc',
  284: 'Pause', 330: 'Num .', 331: 'Num /', 332: 'Num *', 333: 'Num -', 334: 'Num +', 335: 'Num Enter',
  340: 'Shift', 341: 'Ctrl', 342: 'Alt', 343: 'Super', 344: 'Shift', 345: 'Ctrl', 346: 'AltGr', 347: 'Super', 348: 'Menu',
};

/** Short name to show for a key code; `t` translates the few words that need it. */
export function keyName(code: number, t: (key: string) => string): string {
  if (!code) return '—';
  if (code >= MOUSE_BASE) {
    const key = ['mouseLeft', 'mouseRight', 'mouseMiddle', 'mouse4', 'mouse5'][code - MOUSE_BASE];
    return key ? t(key) : '?';
  }
  if ((code >= 65 && code <= 90) || (code >= 48 && code <= 57)) return String.fromCharCode(code);
  if (code >= 290 && code <= 301) return `F${code - 289}`;
  if (code >= 320 && code <= 329) return `Num ${code - 320}`;
  if (code === 32) return t('space');
  return KEY_NAMES[code] ?? `#${code}`;
}
