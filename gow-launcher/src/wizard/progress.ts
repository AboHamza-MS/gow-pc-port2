import type { DestinationReason, IsoReason } from '../lib/types';

/** Weight of each `install.ps1` stage in the bar (the game build dominates the time). */
export const stageWeights = (skipCapture: boolean): number[] =>
  skipCapture ? [0.01, 0.03, 0.02, 0.07, 0.03, 0.83, 0.0, 0.01] : [0.01, 0.03, 0.02, 0.07, 0.03, 0.58, 0.25, 0.01];

/** Overall progress (0-1) from the current stage and the progress inside it (-1 = unknown). */
export function overallProgress(stage: number, stageProgress: number, weights: number[]): number {
  if (stage < 0) return 0;
  let done = 0;
  for (let i = 0; i < Math.min(stage, weights.length); i++) done += weights[i];
  if (stage < weights.length) done += weights[stage] * Math.max(0, stageProgress);
  return Math.min(1, done);
}

export const isoMessageKey: Record<IsoReason, string> = {
  empty: '',
  'not-found': 'wizIsoNotFound',
  unreadable: 'wizIsoUnreadable',
  format: 'wizIsoFormat',
  'other-game': 'wizIsoOtherGame',
  'no-executable': 'wizIsoNoExe',
  modified: 'wizIsoModified',
  'other-edition': 'wizIsoOtherEdition',
  chd: 'wizIsoChd',
};

export const destinationMessageKey: Record<DestinationReason, string> = {
  empty: 'wizDestinationEmpty',
  characters: 'wizDestinationCharacters',
  space: 'wizDestinationSpace',
};

export const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** GOW-Port (0.3.1): a CHD disc image, converted to ISO before installing. */
export const isChd = (path: string) => /\.chd$/i.test(path.trim());
