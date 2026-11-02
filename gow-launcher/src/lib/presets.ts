import type { Settings } from './types';

/**
 * Graphics presets. They are derived, not stored: the Graphics tab shows the preset whose values the four options below
 * already have, and "Custom" otherwise. A preset never touches the display mode, aspect, widescreen, window size,
 * vsync, frame limit, progressive 480p or the graphics card: those are personal preferences.
 */
export type PresetId = 'performance' | 'balanced' | 'quality';

/** The only settings a preset reads and writes. */
export type PresetFields = Pick<Settings, 'internalResolution' | 'filter' | 'sharpness' | 'fxaa'>;

export const PRESET_IDS: PresetId[] = ['performance', 'balanced', 'quality'];

/**
 * Balanced is `Settings::reset_game_options` in gow-core (`settings.rs`) for these four fields, which is also
 * `Settings::default`: a player who never touched the options starts on it. The Rust side is pinned by the test
 * `the_balanced_preset_is_what_a_fresh_install_and_a_reset_give` in `settings.rs`, which holds these same values: change one
 * of the four defaults there and it fails, and `presets.test.ts` keeps this table honest on the TypeScript side.
 *
 * Performance leaves the sharpness alone (`null`): it only matters for the bicubic filter and Performance uses bilinear.
 */
export const PRESETS: Record<PresetId, Omit<PresetFields, 'sharpness'> & { sharpness: number | null }> = {
  performance: { internalResolution: '1', filter: 'bilinear', sharpness: null, fxaa: false },
  balanced: { internalResolution: '2', filter: 'bilinear', sharpness: 0.8, fxaa: true },
  quality: { internalResolution: '3', filter: 'fsr', sharpness: 0.8, fxaa: true },
};

/** The values a preset writes: what the preset fixes and nothing else. */
export function presetChange(id: PresetId): Partial<PresetFields> {
  const { sharpness, ...fixed } = PRESETS[id];
  return sharpness === null ? fixed : { ...fixed, sharpness };
}

/** An empty filter is the bicubic one: `launch.rs` launches it that way. */
const effectiveFilter = (filter: string) => (filter === '' ? 'fsr' : filter);

/**
 * The preset the current options match, or `null` (Custom). The sharpness only counts when the filter is the bicubic one:
 * the slider is locked otherwise and the game ignores the value, so a leftover sharpness is not a customization.
 */
export function activePreset(settings: PresetFields): PresetId | null {
  const filter = effectiveFilter(settings.filter);
  return PRESET_IDS.find((id) => {
    const preset = PRESETS[id];
    if (preset.internalResolution !== settings.internalResolution) return false;
    if (preset.filter !== filter || preset.fxaa !== settings.fxaa) return false;
    return filter !== 'fsr' || preset.sharpness === null || Math.abs(preset.sharpness - settings.sharpness) < 1e-6;
  }) ?? null;
}
