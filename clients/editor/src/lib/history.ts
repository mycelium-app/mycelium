export type History<T> = { past: T[]; present: T; future: T[] };

export const historyLimit = 100;

export function createHistory<T>(present: T): History<T>
{
  return { past: [], present, future: [] };
}

export function commit<T>(history: History<T>, next: T): History<T>
{
  if (next === history.present) return history;

  return { past: [...history.past, history.present].slice(-historyLimit), present: next, future: [] };
}

export function undo<T>(history: History<T>): History<T>
{
  const previous = history.past[history.past.length - 1];

  if (history.past.length === 0) return history;

  return { past: history.past.slice(0, -1), present: previous, future: [history.present, ...history.future] };
}

export function redo<T>(history: History<T>): History<T>
{
  if (history.future.length === 0) return history;

  const [next, ...rest] = history.future;

  return { past: [...history.past, history.present], present: next, future: rest };
}
