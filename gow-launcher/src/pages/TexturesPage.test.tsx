import { describe, expect, it } from 'vitest';
import { packState } from './TexturesPage';
import type { TexturePackInfo } from '../lib/types';

const info = (installed: boolean): TexturePackInfo => ({
  meta: { size: 3_990_828_210, etag: 'etag', uploaded: '2026-10-29T02:58:32.356Z' },
  status: { installed, files: installed ? 2 : 0, bytes: 0, etag: installed ? 'etag' : '', uploaded: '' },
});

describe('texture pack card', () => {
  it('is hidden while custom textures are off', () => {
    expect(packState(false, info(false), true)).toBe('hidden');
    expect(packState(false, info(false), false)).toBe('hidden');
  });

  it('is hidden once the folder already has textures', () => {
    expect(packState(true, info(true), true)).toBe('hidden');
  });

  it('asks for the game first when nothing is installed', () => {
    expect(packState(true, info(false), false)).toBe('ask');
  });

  it('is offered once the game is installed and the folder is empty', () => {
    expect(packState(true, info(false), true)).toBe('offer');
  });

  it('waits for the pack status before deciding', () => {
    expect(packState(true, null, true)).toBe('hidden');
  });
});
