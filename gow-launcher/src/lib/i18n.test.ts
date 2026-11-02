import { describe, expect, it } from 'vitest';
import en from '../i18n/en.json';
import ptBR from '../i18n/pt-BR.json';
import { CHEATS } from '../pages/CheatsPage';
import { actions, groups } from './keys';
import { format, formatDuration, translate, upper } from './i18n';
import type { Language } from './types';

const sources = import.meta.glob(['../**/*.ts', '../**/*.tsx', '!../**/*.test.*'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

const files = import.meta.glob('../i18n/*.json', { import: 'default', eager: true }) as Record<string, Record<string, string>>;
const languages = Object.entries(files).map(([path, table]) => ({ code: /([^/]+)\.json$/.exec(path)![1], table }));

/** The `%1`, `%2`... tokens of a text, sorted, so two texts agree when they substitute the same values. */
const placeholders = (text: string) => (text.match(/%\d+/g) ?? []).sort().join(' ');
const edges = (text: string) => [/^\s*/.exec(text)![0], /\s*$/.exec(text)![0]];

/**
 * What a translation must keep from the English text, one line per problem, so a failure lists all of them at once:
 * the same placeholders, a value where English has one, the same whitespace at both ends, and not the key itself
 * (the text of a key that was never translated).
 */
export function translationProblems(reference: Record<string, string>, code: string, table: Record<string, string>): string[] {
  const problems: string[] = [];
  for (const [key, english] of Object.entries(reference)) {
    const value = table[key];
    if (value === undefined) continue; // the key-set test reports missing keys
    if (placeholders(value) !== placeholders(english)) {
      problems.push(`${code}.${key}: placeholders [${placeholders(value)}] differ from English [${placeholders(english)}]`);
    }
    if (english.trim() !== '' && value.trim() === '') problems.push(`${code}.${key}: empty, English has a text`);
    if (edges(value).join('|') !== edges(english).join('|')) problems.push(`${code}.${key}: leading or trailing whitespace differs from English`);
    if (value === key) problems.push(`${code}.${key}: the value is the key itself`);
  }
  return problems;
}

/** Texts that are still the English word in pt-BR (the GPU kinds: "integrada" and "dedicada" in Portuguese). Known, not fixed here. */
const STILL_UNTRANSLATED = [
  'pt-BR.integrated: the value is the key itself',
  'pt-BR.dedicated: the value is the key itself',
];

describe('translation files', () => {
  it('include English and one file per registered language', () => {
    expect(languages.map((language) => language.code)).toContain('en');
    // `translate` reads `tables[language]`: a file nobody added to the table in i18n.ts throws here instead of being ignored.
    for (const { code, table } of languages) {
      const [key, value] = Object.entries(table)[0];
      const read = () => {
        try {
          return translate(code as Language, key);
        } catch {
          return undefined;
        }
      };
      expect(read(), `${code}.json is not registered in the tables of src/lib/i18n.ts`).toBe(value);
    }
  });

  it('keep the placeholders, the whitespace and a real text in every language', () => {
    const found = languages
      .filter(({ code }) => code !== 'en')
      .flatMap(({ code, table }) => translationProblems(en, code, table));
    expect(found.filter((problem) => !STILL_UNTRANSLATED.includes(problem))).toEqual([]);
    // Translating one of them must also take it off the list.
    expect(STILL_UNTRANSLATED.filter((problem) => !found.includes(problem))).toEqual([]);
  });

  it('notice the mistakes a translator can make', () => {
    const reference = { total: 'Total: %1 of %2', play: ' PLAY', note: 'Hello' };
    expect(translationProblems(reference, 'xx', { total: 'Total: %1', play: ' JOGAR', note: 'Olá' })).toHaveLength(1);
    expect(translationProblems(reference, 'xx', { total: '%2 de %1', play: ' JOGAR', note: 'Olá' })).toEqual([]);
    expect(translationProblems(reference, 'xx', { total: 'Total: %1 %1', play: ' JOGAR', note: 'Olá' })).toHaveLength(1);
    expect(translationProblems(reference, 'xx', { total: '%1 %2', play: 'JOGAR', note: 'Olá' })).toHaveLength(1);
    expect(translationProblems(reference, 'xx', { total: '%1 %2', play: ' JOGAR', note: '' })).toHaveLength(1);
    expect(translationProblems(reference, 'xx', { total: '%1 %2', play: ' JOGAR', note: 'note' })).toHaveLength(1);
    expect(translationProblems(reference, 'xx', { total: '%1 %2', play: ' JOGAR', note: 'Olá' })).toEqual([]);
  });
});

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
      // The failures the launcher explains (`FailureKind::key` in gow-core).
      ...['game', 'installation', 'optimizer', 'texturePack'].map((blocker) => `rebuildBlocked_${blocker}`),
      'failureKitFile', 'failureCmakeUnusable', 'failureDownload', 'failureOpenGl', 'failureMissingDll', 'failureBadImage',
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
