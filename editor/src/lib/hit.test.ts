import { describe, expect, it } from 'vitest';
import type { Item, Page } from './doc';
import { type CursorState, cursorFor, handleCursor, hitItem, visibleHandles } from './hit';

const artboard: Item = { id: 'a1', type: 'artboard', name: 'A', x: 0, y: 0, w: 800, h: 600, html: '' };
const small: Item = { id: 's1', type: 'rect', x: 100, y: 100, w: 4, h: 4, fill: '#000', stroke: null, radius: 0 };
const big: Item = { id: 's2', type: 'rect', x: 200, y: 100, w: 100, h: 100, fill: '#000', stroke: null, radius: 0 };
const stroke: Item = { id: 'd1', type: 'path', points: [[400, 400], [500, 400]], color: '#000', width: 2 };

function page(...items: Item[]): Page
{
  return { id: 'p1', name: 'Page 1', items };
}

describe('hitItem', () =>
{
  it('picks an item over the artboard beneath it', () =>
  {
    expect(hitItem(page(artboard, big), { x: 250, y: 150 }, 1)?.id).toBe('s2');
  });

  it('falls back to the artboard on empty space', () =>
  {
    expect(hitItem(page(artboard, big), { x: 600, y: 500 }, 1)?.id).toBe('a1');
  });

  it('hits a tiny item slightly outside its edge', () =>
  {
    expect(hitItem(page(artboard, small), { x: 109, y: 102 }, 1)?.id).toBe('s1');
  });

  it('keeps the minimum hit area constant on screen when zoomed out', () =>
  {
    expect(hitItem(page(artboard, small), { x: 112, y: 102 }, 0.25)?.id).toBe('s1');
    expect(hitItem(page(artboard, small), { x: 112, y: 102 }, 1)?.id).toBe('a1');
  });

  it('lets the topmost of overlapping items win', () =>
  {
    const under: Item = { ...big, id: 'under' };
    const over: Item = { ...big, id: 'over', x: 250 };

    expect(hitItem(page(under, over), { x: 270, y: 150 }, 1)?.id).toBe('over');
  });

  it('prefers an item under the pointer over a neighbour reached only by padding', () =>
  {
    const neighbour: Item = { ...small, id: 'near', x: 120, y: 100 };
    const body: Item = { ...big, id: 'body', x: 100, y: 100, w: 18, h: 18 };

    expect(hitItem(page(body, neighbour), { x: 110, y: 110 }, 1)?.id).toBe('body');
  });

  it('chooses the nearest padded item when none is directly under the pointer', () =>
  {
    const left: Item = { ...small, id: 'left', x: 100, y: 100 };
    const right: Item = { ...small, id: 'right', x: 112, y: 100 };

    expect(hitItem(page(left, right), { x: 109, y: 102 }, 1)?.id).toBe('right');
  });

  it('hits a thin stroke near the line but not far from it', () =>
  {
    expect(hitItem(page(artboard, stroke), { x: 450, y: 405 }, 1)?.id).toBe('d1');
    expect(hitItem(page(artboard, stroke), { x: 450, y: 420 }, 1)?.id).toBe('a1');
  });

  it('does not treat the bounding box of a stroke as a hit', () =>
  {
    const bent: Item = { id: 'd2', type: 'path', points: [[0, 0], [100, 0], [100, 100]], color: '#000', width: 2 };

    expect(hitItem(page(bent), { x: 20, y: 80 }, 1)).toBeUndefined();
  });

  it('returns nothing on an empty page', () =>
  {
    expect(hitItem(page(), { x: 1, y: 1 }, 1)).toBeUndefined();
  });
});

describe('visibleHandles', () =>
{
  const bounds = (size: number) => ({ x: 0, y: 0, w: size, h: size });

  it('shows all eight handles on a comfortably sized selection', () =>
  {
    expect(visibleHandles(bounds(100), 1)).toHaveLength(8);
  });

  it('shows only corners on a small selection', () =>
  {
    expect(visibleHandles(bounds(30), 1)).toEqual(['nw', 'ne', 'se', 'sw']);
  });

  it('shows none on a tiny selection so neighbours stay clickable', () =>
  {
    expect(visibleHandles(bounds(10), 1)).toEqual([]);
  });

  it('measures size on screen, not on the canvas', () =>
  {
    expect(visibleHandles(bounds(100), 0.1)).toEqual([]);
  });
});

describe('cursors', () =>
{
  const idle: CursorState = { tool: 'select', panning: false, panGesture: false, moving: false, resizeHandle: null, hovered: undefined, selectedId: null, editingId: null, editable: true };

  it('maps every handle to its resize cursor', () =>
  {
    expect(['nw', 'se'].map((handle) => handleCursor(handle as 'nw'))).toEqual(['nwse-resize', 'nwse-resize']);
    expect(['ne', 'sw'].map((handle) => handleCursor(handle as 'ne'))).toEqual(['nesw-resize', 'nesw-resize']);
    expect(['n', 's'].map((handle) => handleCursor(handle as 'n'))).toEqual(['ns-resize', 'ns-resize']);
    expect(['e', 'w'].map((handle) => handleCursor(handle as 'e'))).toEqual(['ew-resize', 'ew-resize']);
  });

  it('keeps the resize cursor for the whole resize gesture', () =>
  {
    expect(cursorFor({ ...idle, resizeHandle: 'ne' })).toBe('nesw-resize');
  });

  it('shows move over the selected item and default over others', () =>
  {
    expect(cursorFor({ ...idle, hovered: big, selectedId: 's2' })).toBe('move');
    expect(cursorFor({ ...idle, hovered: big, selectedId: 's1' })).toBe('default');
  });

  it('shows default over a selected item in view-only mode', () =>
  {
    expect(cursorFor({ ...idle, hovered: big, selectedId: 's2', editable: false })).toBe('default');
  });

  it('shows text over the item being edited', () =>
  {
    const text: Item = { id: 't1', type: 'text', x: 0, y: 0, w: 100, text: 'Hi', size: 16, color: '#000' };

    expect(cursorFor({ ...idle, hovered: text, selectedId: 't1', editingId: 't1' })).toBe('text');
  });

  it('shows crosshair for creation tools and grab for panning', () =>
  {
    expect(cursorFor({ ...idle, tool: 'rect' })).toBe('crosshair');
    expect(cursorFor({ ...idle, panning: true })).toBe('grab');
    expect(cursorFor({ ...idle, panning: true, panGesture: true })).toBe('grabbing');
  });
});
