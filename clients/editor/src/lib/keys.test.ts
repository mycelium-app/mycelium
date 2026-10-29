import { describe, expect, it } from 'vitest';
import { type KeyInput, commandForKey, submitsComment } from './keys';

function press(key: string, modifiers: Partial<KeyInput> = {}): KeyInput
{
  return { key, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...modifiers };
}

describe('keyboard shortcuts', () =>
{
  it.each([['v', 'select'], ['h', 'hand'], ['t', 'text'], ['f', 'artboard'], ['n', 'note'], ['p', 'pen'], ['r', 'rect'], ['o', 'ellipse'], ['c', 'comment']])('%s picks the %s tool', (key, tool) =>
  {
    expect(commandForKey(press(key))).toEqual({ kind: 'tool', tool });
  });

  it('undoes with control or command Z and redoes with shift', () =>
  {
    expect(commandForKey(press('z', { ctrlKey: true }))).toEqual({ kind: 'undo' });
    expect(commandForKey(press('z', { metaKey: true }))).toEqual({ kind: 'undo' });
    expect(commandForKey(press('Z', { ctrlKey: true, shiftKey: true }))).toEqual({ kind: 'redo' });
  });

  it('duplicates with control D', () =>
  {
    expect(commandForKey(press('d', { ctrlKey: true }))).toEqual({ kind: 'duplicate' });
  });

  it('deletes, fits and zooms', () =>
  {
    expect(commandForKey(press('Delete'))).toEqual({ kind: 'delete' });
    expect(commandForKey(press('0'))).toEqual({ kind: 'fit' });
    expect(commandForKey(press('+'))).toEqual({ kind: 'zoomIn' });
    expect(commandForKey(press('-'))).toEqual({ kind: 'zoomOut' });
  });

  it('does not switch tool when a modifier is held', () =>
  {
    expect(commandForKey(press('v', { ctrlKey: true }))).toBeNull();
    expect(commandForKey(press('x'))).toBeNull();
  });
});

describe('comment submit key', () =>
{
  it('submits on Enter', () =>
  {
    expect(submitsComment({ key: 'Enter', shiftKey: false })).toBe(true);
  });

  it('keeps Shift Enter for a new line', () =>
  {
    expect(submitsComment({ key: 'Enter', shiftKey: true })).toBe(false);
  });

  it('does not submit while an input method is composing', () =>
  {
    expect(submitsComment({ key: 'Enter', shiftKey: false, isComposing: true })).toBe(false);
  });
});
