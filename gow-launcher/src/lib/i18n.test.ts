import { describe, expect, it } from 'vitest';
import en from '../i18n/en.json';
import ptBR from '../i18n/pt-BR.json';
import { CHEATS } from '../pages/CheatsPage';
import { actions, groups } from './keys';
import { format, formatDuration, translate, upper } from './i18n';

const sources = import.meta.glob(['../**/*.ts', '../**/*.tsx', '!../**/*.test.*'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

describe('translations', () => {
  it('have the same keys in both languages', () => {
    expect(Object.keys(ptBR).sort()).toEqual(Object.keys(en).sort());
  });

  it('cover every literal key used in the code', () => {
    const used = new Set<string>();
    for (const source of Object.values(sources)) {
      for (const match of source.matchAll(/\b(?:t|f|translate)\(\s*(?:lang,\s*)?'([A-Za-z0-9_]+)'/g)) used.add(match[1]);
      // Keys held in tables: `key: 'tabGraphics'`, `label: ...` and message maps.
      for (const match of source.matchAll(/\b(?:key|text): '((?:tab|wiz)[A-Za-z0-9]+)'/g)) used.add(match[1]);
      for (const match of source.matchAll(/:\s*'(wiz[A-Za-z]+)'/g)) used.add(match[1]);
    }
    const missing = [...used].filter((key) => !(key in en));
    expect(missing).toEqual([]);
    expect(used.size).toBeGreaterThan(100);
  });

  it('cover the dynamic families', () => {
    const keys = [
      ...actions.map((a) => `act_${a.id}`),
      ...groups,
      ...CHEATS.flatMap((id) => [`cheat_${id}`, `cheat_${id}Desc`]),
      ...Array.from({ length: 7 }, (_, i) => `wizStage${i}`),
      'mouseLeft', 'mouseRight', 'mouseMiddle', 'mouse4', 'mouse5', 'space',
      'errorExecutable', 'errorElf', 'errorElfMismatch', 'errorArranque', 'integrated', 'dedicated',
      // The reasons an update waits (`UpdateBlocker`), and the two notes of a copy that cannot update itself.
      ...['game', 'installation', 'optimizer', 'texturePack'].map((blocker) => `updateBlocked_${blocker}`),
      'updatePortableNote', 'updateManualNote',
    ];
    expect(keys.filter((key) => !(key in en))).toEqual([]);
  });
});

describe('formatting', () => {
  it('substitutes placeholders and falls back to the key', () => {
    expect(format('en', 'totalTime', '5 min')).toBe('Total play time: 5 min');
    expect(translate('en', 'no-such-key')).toBe('no-such-key');
    expect(format('pt-BR', 'finishedOk', '2 min')).toContain('2 min');
  });

  it('formats durations', () => {
    expect(formatDuration('en', 45)).toBe('45 s');
    expect(formatDuration('en', 125)).toBe('2 min');
    expect(formatDuration('en', 3 * 3600 + 5 * 60)).toBe('3 h 5 min');
  });

  it('uppercases with the right locale', () => {
    expect(upper('pt-BR', 'configurações')).toBe('CONFIGURAÇÕES');
  });
});
