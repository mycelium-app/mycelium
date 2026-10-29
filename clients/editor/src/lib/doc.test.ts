import { describe, expect, it } from 'vitest';
import { type DesignDoc, addItem, addPage, alignItem, duplicateItem, emptyDesign, findItem, hitTest, itemBounds, moveItem, newItem, nextId, parseDesign, presentOrder, removeItem, resizeItem, simplifyPoints, stackItem } from './doc';

function withArtboard(): DesignDoc
{
  const doc = emptyDesign();

  return addItem(doc, 'p1', newItem(doc, 'artboard', { x: 0, y: 0, w: 400, h: 300 }));
}

describe('parsing', () =>
{
  it('gives an empty text a single blank page', () =>
  {
    expect(parseDesign('')).toEqual(emptyDesign());
  });

  it('fills in missing page ids, names and items', () =>
  {
    const doc = parseDesign('{"pages":[{},{"id":"x","name":"Mine","items":[]}]}');

    expect(doc.pages.map((page) => [page.id, page.name, page.items])).toEqual([['p1', 'Page 1', []], ['x', 'Mine', []]]);
  });
});

describe('adding items', () =>
{
  it('numbers new ids after the highest in use', () =>
  {
    const doc = withArtboard();

    expect(nextId(doc, 'a')).toBe('a2');
    expect(nextId(doc, 'p')).toBe('p2');
  });

  it('names artboards in order', () =>
  {
    const doc = withArtboard();
    const second = newItem(doc, 'artboard', { x: 500, y: 0, w: 100, h: 100 });

    expect(second).toMatchObject({ id: 'a2', name: 'Artboard 2' });
  });

  it('adds a page with the next id and name', () =>
  {
    expect(addPage(emptyDesign()).pages.map((page) => [page.id, page.name])).toEqual([['p1', 'Page 1'], ['p2', 'Page 2']]);
  });
});

describe('moving and resizing', () =>
{
  it('moves a path by moving every point', () =>
  {
    const doc = addItem(emptyDesign(), 'p1', { id: 'd1', type: 'path', points: [[0, 0], [10, 5]], color: '#fff', width: 2 });
    const moved = findItem(moveItem(doc, 'd1', 3, 4), 'd1');

    expect(moved).toMatchObject({ points: [[3, 4], [13, 9]] });
  });

  it('resizes from the south east corner without moving the origin', () =>
  {
    const resized = findItem(resizeItem(withArtboard(), 'a1', 'se', 50, 20), 'a1');

    expect(resized).toMatchObject({ x: 0, y: 0, w: 450, h: 320 });
  });

  it('resizes from the north west corner keeping the opposite corner fixed', () =>
  {
    const resized = findItem(resizeItem(withArtboard(), 'a1', 'nw', 100, 50), 'a1');

    expect(resized).toMatchObject({ x: 100, y: 50, w: 300, h: 250 });
  });

  it('never shrinks an item below the minimum size', () =>
  {
    const resized = findItem(resizeItem(withArtboard(), 'a1', 'e', -1000, 0), 'a1');

    expect(resized).toMatchObject({ w: 8 });
  });
});

describe('deleting and duplicating', () =>
{
  it('removes an item from its page', () =>
  {
    expect(findItem(removeItem(withArtboard(), 'a1'), 'a1')).toBeUndefined();
  });

  it('duplicates an item offset with a new id, renaming artboards', () =>
  {
    const result = duplicateItem(withArtboard(), 'a1');
    const copy = findItem(result.doc, result.id!);

    expect(copy).toMatchObject({ id: 'a2', name: 'Artboard 1 copy', x: 24, y: 24 });
  });

  it('reports no id when duplicating a missing item', () =>
  {
    expect(duplicateItem(emptyDesign(), 'nope').id).toBeNull();
  });
});

describe('layout', () =>
{
  it('aligns an item to the right edge of its artboard', () =>
  {
    const base = withArtboard();
    const doc = addItem(base, 'p1', newItem(base, 'rect', { x: 10, y: 10, w: 100, h: 50 }));

    expect(findItem(alignItem(doc, 's1', 'right'), 's1')).toMatchObject({ x: 300 });
  });

  it('centres text using its own width', () =>
  {
    const base = withArtboard();
    const doc = addItem(base, 'p1', newItem(base, 'text', { x: 10, y: 10, w: 200, h: 24 }));

    expect(findItem(alignItem(doc, 't1', 'center'), 't1')).toMatchObject({ x: 100 });
  });

  it('brings an item to the front and sends it to the back', () =>
  {
    const base = withArtboard();
    const doc = addItem(base, 'p1', newItem(base, 'rect', { x: 0, y: 0, w: 10, h: 10 }));

    expect(stackItem(doc, 'a1', 'front').pages[0].items.map((item) => item.id)).toEqual(['s1', 'a1']);
    expect(stackItem(doc, 's1', 'back').pages[0].items.map((item) => item.id)).toEqual(['s1', 'a1']);
  });

  it('prefers a loose item over the artboard beneath it when hit testing', () =>
  {
    const base = withArtboard();
    const doc = addItem(base, 'p1', newItem(base, 'rect', { x: 10, y: 10, w: 100, h: 50 }));

    expect(hitTest(doc.pages[0], 20, 20)?.id).toBe('s1');
    expect(hitTest(doc.pages[0], 300, 250)?.id).toBe('a1');
    expect(hitTest(doc.pages[0], 900, 900)).toBeUndefined();
  });

  it('orders artboards top to bottom then left to right for presenting', () =>
  {
    const base = emptyDesign();
    const one = addItem(base, 'p1', { ...newItem(base, 'artboard', { x: 500, y: 0, w: 10, h: 10 }), id: 'a1' });
    const two = addItem(one, 'p1', { ...newItem(one, 'artboard', { x: 0, y: 0, w: 10, h: 10 }), id: 'a2' });

    expect(presentOrder(two.pages[0]).map((artboard) => artboard.id)).toEqual(['a2', 'a1']);
  });

  it('measures a path by its points', () =>
  {
    expect(itemBounds({ id: 'd1', type: 'path', points: [[2, 3], [12, 8]], color: '#fff', width: 2 })).toEqual({ x: 2, y: 3, w: 10, h: 5 });
  });
});

describe('freehand paths', () =>
{
  it('drops points closer than the minimum distance but keeps the end', () =>
  {
    expect(simplifyPoints([[0, 0], [1, 0], [5, 0], [6, 0]], 4)).toEqual([[0, 0], [5, 0], [6, 0]]);
  });
});
