/** Gigabytes with two decimals, in the player's language. */
export const gigabytes = (bytes: number, locale: string) =>
  (bytes / 1024 ** 3).toLocaleString(locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/**
 * The progress the bar shows: the one the backend reports for the whole installation (download then extraction), and
 * never less than what was already shown, so a late or reordered event cannot send the bar backwards.
 */
export function nextOverall(current: number, reported: number): number {
  return Math.min(1, Math.max(current, Number.isFinite(reported) ? reported : 0));
}
