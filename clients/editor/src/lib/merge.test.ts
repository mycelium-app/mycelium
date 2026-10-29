import { describe, expect, it } from 'vitest';
import { type DesignDoc, addItem, emptyDesign, findItem, moveItem, newItem, removeItem, updateItem } from './doc';
import { hasLocalChanges, mergeIncoming } from './merge';

function base(): DesignDoc
{
  const doc = emptyDesign();
  const withArtboard = addItem(doc, 'p1', newItem(doc, 'artboard', { x: 0, y: 0, w: 400, h: 300 }));

  return addItem(withArtboard, 'p1', newItem(withArtboard, 'rect', { x: 10, y: 10, w: 100, h: 50 }));
}

describe('merging an agent version with local edits', () =>
{
  it('takes the incoming document when there are no local edits', () =>
  {
    const start = base();
    const incoming = updateItem(start, 'a1', { html: '<p>new</p>' });

    expect(mergeIncoming(start, start, incoming)).toEqual(incoming);
  });

  it('reapplies a local move on top of an agent change to another item', () =>
  {
    const start = base();
    const local = moveItem(start, 's1', 5, 5);
    const incoming = updateItem(start, 'a1', { html: '<p>new</p>' });
    const merged = mergeIncoming(start, local, incoming);

    expect(findItem(merged, 's1')).toMatchObject({ x: 15, y: 15 });
    expect(findItem(merged, 'a1')).toMatchObject({ html: '<p>new</p>' });
  });

  it('keeps the agent change to a field the person did not touch on the same item', () =>
  {
    const start = base();
    const local = moveItem(start, 'a1', 20, 0);
    const incoming = updateItem(start, 'a1', { html: '<p>new</p>' });

    expect(findItem(mergeIncoming(start, local, incoming), 'a1')).toMatchObject({ x: 20, html: '<p>new</p>' });
  });

  it('keeps an item the person added locally', () =>
  {
    const start = base();
    const local = addItem(start, 'p1', newItem(start, 'note', { x: 0, y: 0, w: 240, h: 120 }));
    const merged = mergeIncoming(start, local, start);

    expect(findItem(merged, 'n1')).toBeDefined();
  });

  it('gives a local item a fresh id when the agent added one with the same id', () =>
  {
    const start = base();
    const local = addItem(start, 'p1', newItem(start, 'note', { x: 0, y: 0, w: 240, h: 120 }));
    const incoming = addItem(start, 'p1', { ...newItem(start, 'note', { x: 50, y: 50, w: 240, h: 120 }), text: 'agent' });
    const merged = mergeIncoming(start, local, incoming);
    const notes = merged.pages[0].items.filter((item) => item.type === 'note');

    expect(notes.map((note) => note.id)).toEqual(['n1', 'n2']);
  });

  it('keeps a local deletion', () =>
  {
    const start = base();
    const local = removeItem(start, 's1');

    expect(findItem(mergeIncoming(start, local, start), 's1')).toBeUndefined();
  });

  it('reports whether local edits remain', () =>
  {
    const start = base();

    expect(hasLocalChanges(start, start)).toBe(false);
    expect(hasLocalChanges(start, moveItem(start, 's1', 1, 0))).toBe(true);
  });
});
