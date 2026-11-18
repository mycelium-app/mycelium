import { describe, expect, it } from 'vitest';
import { clampZoom, fitBounds, maximumZoom, minimumZoom, panBy, screenToWorld, unionBounds, wheelPan, zoomAt } from './viewport';

describe('viewport', () =>
{
  it('keeps the world point under the cursor fixed while zooming', () =>
  {
    const start = { x: 40, y: 20, zoom: 1 };
    const anchor = { x: 300, y: 200 };
    const before = screenToWorld(start, anchor);
    const after = screenToWorld(zoomAt(start, anchor, 2), anchor);

    expect(after.x).toBeCloseTo(before.x);
    expect(after.y).toBeCloseTo(before.y);
  });

  it('clamps zoom to its limits', () =>
  {
    expect(clampZoom(100)).toBe(maximumZoom);
    expect(clampZoom(0)).toBe(minimumZoom);
  });

  it('pans by a screen offset', () =>
  {
    expect(panBy({ x: 1, y: 2, zoom: 2 }, 10, -5)).toEqual({ x: 11, y: -3, zoom: 2 });
  });

  it('pans sideways for a horizontal trackpad swipe', () =>
  {
    expect(wheelPan({ deltaX: 30, deltaY: 0, shiftKey: false })).toEqual({ x: -30, y: 0 });
  });

  it('pans sideways for a shifted wheel turn that arrives as vertical', () =>
  {
    expect(wheelPan({ deltaX: 0, deltaY: 40, shiftKey: true })).toEqual({ x: -40, y: 0 });
    expect(wheelPan({ deltaX: 40, deltaY: 0, shiftKey: true })).toEqual({ x: -40, y: 0 });
  });

  it('pans in both directions for a diagonal swipe', () =>
  {
    expect(wheelPan({ deltaX: 12, deltaY: -20, shiftKey: false })).toEqual({ x: -12, y: 20 });
  });

  it('fits bounds in the middle of the canvas and never zooms in past 100 per cent', () =>
  {
    const fitted = fitBounds({ x: 0, y: 0, w: 100, h: 100 }, { w: 1000, h: 800 });

    expect(fitted.zoom).toBe(1);
    expect(fitted.x).toBe(450);
    expect(fitted.y).toBe(350);
  });

  it('zooms out to fit bounds larger than the canvas', () =>
  {
    expect(fitBounds({ x: 0, y: 0, w: 2000, h: 1000 }, { w: 1000, h: 800 }, 0).zoom).toBe(0.5);
  });

  it('centres the origin when there is nothing to fit', () =>
  {
    expect(fitBounds(null, { w: 1000, h: 800 })).toEqual({ x: 500, y: 400, zoom: 1 });
  });

  it('unions bounds', () =>
  {
    expect(unionBounds([{ x: 0, y: 0, w: 10, h: 10 }, { x: 20, y: 5, w: 10, h: 10 }])).toEqual({ x: 0, y: 0, w: 30, h: 15 });
    expect(unionBounds([])).toBeNull();
  });
});
