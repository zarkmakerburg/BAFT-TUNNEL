/** Types for `history-view.mjs`. */

export type HistoryNotice = 'ok' | 'empty' | 'unavailable' | 'error';

export function longHistoryNotice(
  state: string | undefined,
  sampleCount: number,
): HistoryNotice;
