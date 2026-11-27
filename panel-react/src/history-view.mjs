/**
 * What the long (24 h) range should say. A missing or failed history source is not an empty
 * range: only a source that answered `ok` with zero samples may say "no samples".
 */
export function longHistoryNotice(state, sampleCount) {
  if (state === 'error') return 'error';
  if (state !== 'ok') return 'unavailable';
  return sampleCount > 0 ? 'ok' : 'empty';
}
