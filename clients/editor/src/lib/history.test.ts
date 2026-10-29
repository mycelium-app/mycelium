import { describe, expect, it } from 'vitest';
import { commit, createHistory, historyLimit, redo, undo } from './history';

describe('history', () =>
{
  it('undoes and redoes a committed change', () =>
  {
    const changed = commit(createHistory('a'), 'b');
    const back = undo(changed);

    expect(back.present).toBe('a');
    expect(redo(back).present).toBe('b');
  });

  it('clears the redo stack when a new change is committed after an undo', () =>
  {
    const back = undo(commit(createHistory('a'), 'b'));

    expect(commit(back, 'c').future).toEqual([]);
  });

  it('ignores committing the same value and undoing with nothing to undo', () =>
  {
    const start = createHistory('a');

    expect(commit(start, 'a')).toBe(start);
    expect(undo(start)).toBe(start);
    expect(redo(start)).toBe(start);
  });

  it('keeps no more than the history limit', () =>
  {
    let history = createHistory(0);

    for (let step = 1; step <= historyLimit + 20; step += 1)
    {
      history = commit(history, step);
    }

    expect(history.past).toHaveLength(historyLimit);
  });
});
