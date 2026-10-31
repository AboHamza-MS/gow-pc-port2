import { describe, expect, it } from 'vitest';
import { fileName, overallProgress, stageWeights } from './progress';

describe('installation progress', () => {
  it('weights add up to one', () => {
    for (const skip of [true, false]) {
      expect(stageWeights(skip).reduce((a, b) => a + b, 0)).toBeCloseTo(1, 5);
    }
  });

  it('is zero before the first stage and one after the last', () => {
    const weights = stageWeights(true);
    expect(overallProgress(-1, 0.5, weights)).toBe(0);
    expect(overallProgress(8, 0, weights)).toBeCloseTo(1, 5);
  });

  it('adds the finished stages and the fraction of the current one', () => {
    const weights = stageWeights(true);
    // Stage 5 (the game build) at 50 %: everything before it plus half of 0.83.
    expect(overallProgress(5, 0.5, weights)).toBeCloseTo(0.01 + 0.03 + 0.02 + 0.07 + 0.03 + 0.415, 5);
    // An indeterminate stage (-1) counts as nothing done inside it.
    expect(overallProgress(5, -1, weights)).toBeCloseTo(0.16, 5);
  });

  it('shows the file name of a Windows or POSIX path', () => {
    expect(fileName('D:\\Games\\God of War.iso')).toBe('God of War.iso');
    expect(fileName('/home/u/god.iso')).toBe('god.iso');
  });
});
