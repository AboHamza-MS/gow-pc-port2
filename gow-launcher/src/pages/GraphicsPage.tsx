import { useMemo } from 'react';
import { Expand, Grid2x2, Image, Maximize, Monitor, RectangleHorizontal, SlidersHorizontal, SquareDashed, Film, StretchHorizontal, TriangleAlert } from 'lucide-react';
import { GroupTitle, Note, Select, SettingRow, Slider, Switch, type Option } from '../components/ui';
import { useApp } from '../state/app';
import { activePreset, presetChange, type PresetId } from '../lib/presets';
import type { DisplayInfo } from '../lib/types';

const SCALES = [1, 1.5, 2, 2.5, 3, 4, 5, 6];
/** `Settings::default` in gow-core: the multiple the option list marks as the default. */
const DEFAULT_SCALE = '2';
/**
 * Lines of each render multiple and the tier they are named after, so the option reads like
 * "2× (~720px/HD)" instead of the raw pixel count. 360 lines is the baseline of the names.
 */
const TIERS: Array<[number, string]> = [
  [540, 'SD'], [720, 'HD'], [900, 'HD+'], [1080, 'FHD'], [1440, 'QHD'], [1800, 'QHD+'], [2160, '4K UHD'],
];
const tierFor = (lines: number) => TIERS.find(([value]) => value === lines)?.[1] ?? `${lines}p`;

const COMMON_RESOLUTIONS: Array<[number, number]> = [
  [1024, 768], [1280, 960], [1440, 1080], [1600, 1200], [1920, 1440], [2880, 2160],
  [1152, 648], [1280, 720], [1366, 768], [1600, 900], [1920, 1080], [2560, 1440], [3200, 1800], [3840, 2160],
  [1280, 800], [1440, 900], [1680, 1050], [1920, 1200], [2560, 1600], [3840, 2400],
  [2560, 1080], [3440, 1440], [3840, 1600], [5120, 2160],
];

/** "Native (PS2)" for 1×, "4× Native (~1440px/QHD)" for the rest, plus the default marker. */
export const scaleText = (n: number, t: (key: string) => string, f: (key: string, ...values: Array<string | number>) => string) => {
  const lines = 360 * n;
  const text = n === 1 ? t('scaleNative') : f('scaleLabel', n, lines, tierFor(lines));
  return String(n) === DEFAULT_SCALE ? `${text} · ${t('scaleDefault')}` : text;
};

/** Numeric ratio of the chosen aspect ("16:9"); with no choice, the screen's. */
function aspectRatio(aspect: string, display: DisplayInfo): number {
  const [w, h] = aspect.split(':').map(parseFloat);
  return w > 0 && h > 0 ? w / h : display.widthPx / display.heightPx;
}

/** Closest aspect the port offers for this display (`DisplayInfo::default_aspect_ratio` on the Rust side). */
function closestAspect(display: DisplayInfo): string {
  const ratio = display.widthPx / Math.max(1, display.heightPx);
  if (ratio >= 2.2) return '21:9';
  if (ratio >= 1.7) return '16:9';
  if (ratio >= 1.5) return '16:10';
  return '4:3';
}

/** Only the resolutions of the chosen aspect that fit on the screen, smallest first. */
export function resolutionsFor(aspect: string, display: DisplayInfo, native: string): Array<Option & { area: number }> {
  const target = aspectRatio(aspect, display);
  const items: Array<Option & { area: number }> = [];
  const include = (w: number, h: number) => {
    if (w > display.widthPx || h > display.heightPx || Math.abs(w / h - target) > 0.06) return;
    const value = `${w}x${h}`;
    if (items.some((item) => item.value === value)) return;
    const isNative = w === display.widthPx && h === display.heightPx;
    items.push({ text: `${w} × ${h}${isNative ? `  (${native})` : ''}`, value, area: w * h });
  };
  COMMON_RESOLUTIONS.forEach(([w, h]) => include(w, h));
  include(display.widthPx, display.heightPx);
  return items.sort((a, b) => a.area - b.area);
}

export function GraphicsPage() {
  const { settings, patch, info, t, f } = useApp();
  const { display } = info;
  const fullscreen = settings.displayMode === 'fullscreen';
  const screenRatio = display.widthPx / display.heightPx;
  const resolutionOptions = useMemo(
    () => resolutionsFor(settings.aspectMode, display, t('native')),
    [settings.aspectMode, display, t],
  );

  const screenTag = (ratio: number) => (Math.abs(ratio - screenRatio) < 0.04 ? `  ·  ${t('screenAspect')}` : '');

  /**
   * Changes the aspect and moves the window to the largest resolution of that aspect when the current one does not
   * belong to it. Any explicit aspect is presented with preserved geometry.
   */
  const changeAspect = (aspect: string, extra: Parameters<typeof patch>[0] = {}) => {
    const change: Parameters<typeof patch>[0] = { aspectMode: aspect, stretch: false, ...extra };
    const options = resolutionsFor(aspect, display, t('native'));
    if (options.length > 0 && !options.some((o) => o.value === `${settings.widthPx}x${settings.heightPx}`)) {
      const [w, h] = String(options[options.length - 1].value).split('x').map(Number);
      change.widthPx = w;
      change.heightPx = h;
    }
    patch(change);
  };

  /**
   * The widescreen switch: on, PCSX2's patch widens the camera (Hor+) to the chosen aspect; off, the game draws its
   * original 4:3 picture and the window shows side bars. Turning it on with a 4:3 aspect (or none) picks the display's
   * own aspect, otherwise there would be nothing to widen.
   */
  const changeWidescreen = (widescreen: boolean) => {
    if (!widescreen) {
      patch({ widescreen });
      return;
    }
    const current = settings.aspectMode;
    changeAspect(current && current !== '4:3' ? current : closestAspect(display), { widescreen });
  };

  const validScale = SCALES.map(String).includes(settings.internalResolution) ? settings.internalResolution : '2';
  const scaleOptions: Option[] = SCALES.map((n) => ({
    text: scaleText(n, t, f),
    value: String(n),
    detail: n === 2 ? t('internalRecommended') : n >= 4 ? t('internalHeavy') : undefined,
  }));

  const preset = activePreset(settings);
  const presetOptions: Array<Option<PresetId | 'custom'>> = [
    { text: t('presetPerformance'), value: 'performance' },
    { text: t('presetBalanced'), value: 'balanced' },
    { text: t('presetQuality'), value: 'quality' },
    { text: t('presetCustom'), value: 'custom', disabled: true },
  ];

  return (
    <div className="flex flex-col gap-1">
      <GroupTitle>{t('groupDisplay')}</GroupTitle>
      <SettingRow icon={Monitor} title={t('modeValue')} description={t('modeDesc')}>
        <Select
          label={t('modeValue')}
          options={[{ text: t('fullscreen'), value: 'fullscreen' }, { text: t('window'), value: 'window' }]}
          value={settings.displayMode}
          onChange={(displayMode) => patch({ displayMode })}
        />
      </SettingRow>
      <SettingRow icon={StretchHorizontal} title={t('widescreen')} description={t('widescreenDesc')}>
        <Switch label={t('widescreen')} checked={settings.widescreen} onChange={changeWidescreen} />
      </SettingRow>
      <SettingRow
        icon={RectangleHorizontal}
        title={t('aspectMode')}
        description={t('aspectDesc')}
        disabled={!settings.widescreen}
      >
        <Select
          label={t('aspectMode')}
          disabled={!settings.widescreen}
          options={[
            { text: t('aspect43') + screenTag(4 / 3), value: '4:3' },
            { text: t('aspect169') + screenTag(16 / 9), value: '16:9' },
            { text: t('aspect1610') + screenTag(16 / 10), value: '16:10' },
            { text: t('aspect219') + screenTag(21 / 9), value: '21:9' },
          ]}
          value={settings.aspectMode}
          onChange={changeAspect}
        />
      </SettingRow>
      <SettingRow icon={Maximize} title={t('resolution')} description={fullscreen ? t('fullResolution') : t('windowResolution')}>
        <Select
          label={t('resolution')}
          disabled={fullscreen}
          options={fullscreen ? [{ text: `${display.widthPx} × ${display.heightPx}  (${t('native')})`, value: 'native' }] : resolutionOptions}
          value={fullscreen ? 'native' : `${settings.widthPx}x${settings.heightPx}`}
          onChange={(value) => {
            const [widthPx, heightPx] = String(value).split('x').map(Number);
            patch({ widthPx, heightPx });
          }}
        />
      </SettingRow>
      <SettingRow icon={Expand} title={t('stretch')} description={t('stretchDesc')} disabled={settings.aspectMode.length > 0}>
        <Switch label={t('stretch')} checked={settings.aspectMode.length === 0 && settings.stretch} onChange={(stretch) => patch({ stretch })} />
      </SettingRow>
      <GroupTitle>{t('groupImage')}</GroupTitle>
      <SettingRow icon={SlidersHorizontal} title={t('presetTitle')} description={t('presetDesc')}>
        <Select
          label={t('presetTitle')}
          options={presetOptions}
          value={preset ?? 'custom'}
          onChange={(id) => id !== 'custom' && patch(presetChange(id))}
        />
      </SettingRow>
      <SettingRow icon={Grid2x2} title={t('internal')} description={t('internalDesc')}>
        <Select
          label={t('internal')}
          options={scaleOptions}
          value={validScale}
          onChange={(internalResolution) => patch({ internalResolution })}
        />
      </SettingRow>
      {parseFloat(validScale) >= 3 && (
        <Note tone="warn" title={t('scaleWarningTitle')}>
          {t('scaleWarning')}
        </Note>
      )}
      <SettingRow icon={Image} title={t('scaling')} description={t('scalingDesc')}>
        <Select
          label={t('scaling')}
          options={[
            { text: t('filterFsr'), value: 'fsr' },
            { text: t('filterBilinear'), value: 'bilinear' },
            { text: t('filterPixel'), value: 'pixel' },
          ]}
          value={settings.filter}
          onChange={(filter) => patch({ filter })}
        />
      </SettingRow>
      <SettingRow icon={TriangleAlert} title={t('sharpness')} description={t('sharpnessDesc')} disabled={settings.filter !== 'fsr'}>
        <Slider label={t('sharpness')} from={0} to={1} step={0.05} value={settings.sharpness} onChange={(sharpness) => patch({ sharpness })} />
      </SettingRow>
      <SettingRow icon={SquareDashed} title={t('fxaa')} description={t('fxaaDesc')}>
        <Switch label={t('fxaa')} checked={settings.fxaa} onChange={(fxaa) => patch({ fxaa })} />
      </SettingRow>
      <SettingRow icon={Film} title={t('progressive')} description={t('progressiveDesc')}>
        <Switch label={t('progressive')} checked={settings.progressive480p} onChange={(progressive480p) => patch({ progressive480p })} />
      </SettingRow>
    </div>
  );
}
