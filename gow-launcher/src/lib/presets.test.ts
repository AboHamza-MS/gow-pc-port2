import { describe, expect, it } from 'vitest';
import { activePreset, presetChange, PRESET_IDS, PRESETS, type PresetFields } from './presets';
import type { Settings } from './types';

/** `Settings::default` and `Settings::reset_game_options` in `gow-core/src/settings.rs`, for the four fields a preset owns. */
const RUST_DEFAULTS: PresetFields = { internalResolution: '2', filter: 'bilinear', sharpness: 0.8, fxaa: true };

/** The rest of the options a preset must leave alone, with values none of the presets would write. */
const OTHERS = {
  displayMode: 'window', aspectMode: '21:9', widescreen: false, stretch: true, widthPx: 1600, heightPx: 900, vsync: true, fpsLimit: 30,
  progressive480p: false, gpu: 'dedicated', showFps: true,
} as const;

const base = (extra: Partial<Settings> = {}) => ({ ...RUST_DEFAULTS, ...OTHERS, ...extra }) as Settings;

describe('graphics presets', () => {
  it('writes the preset and reads it back', () => {
    for (const id of PRESET_IDS) {
      const settings = { ...base({ internalResolution: '5', filter: 'pixel', sharpness: 0.3, fxaa: id === 'performance' }), ...presetChange(id) };
      expect(activePreset(settings)).toBe(id);
    }
  });

  it('Balanced is what a new installation and "Restore defaults" use', () => {
    expect(presetChange('balanced')).toEqual(RUST_DEFAULTS);
    expect(activePreset(RUST_DEFAULTS)).toBe('balanced');
  });

  it('only ever changes its four options', () => {
    for (const id of PRESET_IDS) {
      const changed = Object.keys(presetChange(id));
      expect(changed.every((key) => ['internalResolution', 'filter', 'sharpness', 'fxaa'].includes(key))).toBe(true);
      const after = { ...base(), ...presetChange(id) };
      for (const [key, value] of Object.entries(OTHERS)) expect(after[key as keyof typeof OTHERS]).toBe(value);
    }
  });

  it('turns into Custom as soon as any one option differs', () => {
    for (const id of PRESET_IDS) {
      const applied = { ...base(), ...presetChange(id) };
      expect(activePreset({ ...applied, internalResolution: '4' })).toBeNull();
      expect(activePreset({ ...applied, internalResolution: 'auto' })).toBeNull();
      expect(activePreset({ ...applied, filter: applied.filter === 'fsr' ? 'bilinear' : 'fsr' })).not.toBe(id);
      expect(activePreset({ ...applied, filter: 'pixel' })).toBeNull();
      expect(activePreset({ ...applied, fxaa: !applied.fxaa })).not.toBe(id);
    }
    const quality = { ...base(), ...presetChange('quality') };
    expect(activePreset({ ...quality, sharpness: 0.5 })).toBeNull();
  });

  it('ignores the sharpness while the filter does not use it', () => {
    // The slider is locked unless the filter is bicubic, so a leftover value is not a customization.
    expect(activePreset({ ...RUST_DEFAULTS, sharpness: 0.2 })).toBe('balanced');
    expect(activePreset({ ...RUST_DEFAULTS, internalResolution: '1', fxaa: false, sharpness: 0.2 })).toBe('performance');
  });

  it('reads an empty filter as the bicubic one, as the launch does', () => {
    expect(activePreset({ ...PRESETS.quality, filter: '', sharpness: 0.8 })).toBe('quality');
  });

  it('keeps the Performance preset from touching the sharpness', () => {
    expect(presetChange('performance')).not.toHaveProperty('sharpness');
  });
});
