import type { Bounds } from './doc';

export type Viewport = { x: number; y: number; zoom: number };
export type Point = { x: number; y: number };

export const minimumZoom = 0.05;
export const maximumZoom = 8;
export const zoomStep = 1.25;

export function clampZoom(zoom: number): number
{
  return Math.min(maximumZoom, Math.max(minimumZoom, zoom));
}

export function screenToWorld(viewport: Viewport, point: Point): Point
{
  return { x: (point.x - viewport.x) / viewport.zoom, y: (point.y - viewport.y) / viewport.zoom };
}

export function zoomAt(viewport: Viewport, anchor: Point, factor: number): Viewport
{
  const zoom = clampZoom(viewport.zoom * factor);
  const world = screenToWorld(viewport, anchor);

  return { zoom, x: anchor.x - world.x * zoom, y: anchor.y - world.y * zoom };
}

export function panBy(viewport: Viewport, dx: number, dy: number): Viewport
{
  return { ...viewport, x: viewport.x + dx, y: viewport.y + dy };
}

export function fitBounds(bounds: Bounds | null, size: { w: number; h: number }, padding = 64): Viewport
{
  if (!bounds || bounds.w <= 0 || bounds.h <= 0)
  {
    return { x: size.w / 2, y: size.h / 2, zoom: 1 };
  }

  const zoom = clampZoom(Math.min((size.w - padding * 2) / bounds.w, (size.h - padding * 2) / bounds.h, 1));

  return { zoom, x: (size.w - bounds.w * zoom) / 2 - bounds.x * zoom, y: (size.h - bounds.h * zoom) / 2 - bounds.y * zoom };
}

export function unionBounds(all: Bounds[]): Bounds | null
{
  if (all.length === 0) return null;

  const x = Math.min(...all.map((bounds) => bounds.x));
  const y = Math.min(...all.map((bounds) => bounds.y));

  return { x, y, w: Math.max(...all.map((bounds) => bounds.x + bounds.w)) - x, h: Math.max(...all.map((bounds) => bounds.y + bounds.h)) - y };
}

export function wheelZoomFactor(deltaY: number): number
{
  return Math.exp(-deltaY * 0.01);
}
