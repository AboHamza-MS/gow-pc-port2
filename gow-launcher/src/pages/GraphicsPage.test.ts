import { describe, expect, it } from 'vitest';
import { closestAspect, resolutionsFor } from './GraphicsPage';

const display = (widthPx: number, heightPx: number) => ({ widthPx, heightPx, hz: 60 });

describe('closestAspect', () => {
  it('follows the display, like DisplayInfo::default_aspect_ratio in gow-core', () => {
    expect(closestAspect(display(5120, 1440))).toBe('32:9');
    expect(closestAspect(display(3840, 1080))).toBe('32:9');
    expect(closestAspect(display(3440, 1440))).toBe('21:9');
    expect(closestAspect(display(2560, 1080))).toBe('21:9');
    expect(closestAspect(display(1920, 1080))).toBe('16:9');
    expect(closestAspect(display(1920, 1200))).toBe('16:10');
    expect(closestAspect(display(1024, 768))).toBe('4:3');
  });
});

describe('resolutionsFor 32:9', () => {
  it('offers the 32:9 resolutions that fit the screen, smallest first, and nothing of another aspect', () => {
    const values = resolutionsFor('32:9', display(5120, 1440), 'native').map((option) => option.value);
    expect(values).toEqual(['3840x1080', '5120x1440']);
  });

  it('marks the native resolution', () => {
    const native = resolutionsFor('32:9', display(5120, 1440), 'native').find((option) => option.value === '5120x1440');
    expect(native?.text).toContain('(native)');
  });

  it('does not offer 21:9 resolutions for 32:9 or the other way around', () => {
    const wide = display(5120, 2160);
    expect(resolutionsFor('21:9', wide, 'native').map((o) => o.value)).not.toContain('5120x1440');
    expect(resolutionsFor('32:9', wide, 'native').map((o) => o.value)).not.toContain('5120x2160');
  });
});
