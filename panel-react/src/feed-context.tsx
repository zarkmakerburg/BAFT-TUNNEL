import { createContext, useContext } from 'react';
import type { Feed } from './app';

export const FeedCtx = createContext<Feed | null>(null);
export const useData = (): Feed => {
  const f = useContext(FeedCtx);
  if (!f) throw new Error('FeedCtx missing');
  return f;
};
