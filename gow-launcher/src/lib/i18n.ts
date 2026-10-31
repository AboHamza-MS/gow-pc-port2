import en from '../i18n/en.json';
import ptBR from '../i18n/pt-BR.json';
import type { Language } from './types';

const tables: Record<Language, Record<string, string>> = { en, 'pt-BR': ptBR };

export function asLanguage(value: string): Language {
  return value === 'pt-BR' ? 'pt-BR' : 'en';
}

/** Text for `key`; falls back to English, then to the key itself. */
export function translate(language: Language, key: string): string {
  return tables[language][key] ?? tables.en[key] ?? key;
}

/** `translate` with %1, %2 substituted (first occurrence each, as the QML launcher did). */
export function format(language: Language, key: string, ...values: Array<string | number>): string {
  return values.reduce<string>((text, value, index) => text.replace(`%${index + 1}`, String(value)), translate(language, key));
}

export function formatDuration(language: Language, seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours > 0) return format(language, 'hoursMinutes', hours, minutes);
  if (minutes > 0) return format(language, 'minutes', minutes);
  return format(language, 'seconds', seconds);
}

export const upper = (language: Language, text: string) => text.toLocaleUpperCase(language === 'pt-BR' ? 'pt-BR' : 'en-US');

/** Which interface language the system suggests. */
export function systemLocale(): string {
  return typeof navigator === 'undefined' ? 'en' : navigator.language || 'en';
}
