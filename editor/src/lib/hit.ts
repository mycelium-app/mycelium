import { type Bounds, type Handle, type Item, type Page, itemBounds } from './doc';
import type { Point } from './viewport';
import type { Tool } from './keys';

export const minimumHitSize = 14;
export const minimumHandleSize = 48;
export const minimumHandleCornerSize = 20;

const cornerHandles: Handle[] = ['nw', 'ne', 'se', 'sw'];
const allHandles: Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'];

const handleCursors: Record<Handle, string> = {
  nw: 'nwse-resize',
  se: 'nwse-resize',
  ne: 'nesw-resize',
  sw: 'nesw-resize',
  n: 'ns-resize',
  s: 'ns-resize',
  e: 'ew-resize',
  w: 'ew-resize',
};

export type CursorState = {
  tool: Tool;
  panning: boolean;
  panGesture: boolean;
  moving: boolean;
  resizeHandle: Handle | null;
  hovered: Item | undefined;
  selectedId: string | null;
  editingId: string | null;
  editable: boolean;
};

export function handleCursor(handle: Handle): string
{
  return handleCursors[handle];
}

export function visibleHandles(bounds: Bounds, zoom: number): Handle[]
{
  const smallest = Math.min(bounds.w, bounds.h) * zoom;

  if (smallest >= minimumHandleSize) return allHandles;
  if (smallest >= minimumHandleCornerSize) return cornerHandles;

  return [];
}

function distanceToSegment(point: Point, a: [number, number], b: [number, number]): number
{
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const lengthSquared = dx * dx + dy * dy;
  const t = lengthSquared === 0 ? 0 : Math.max(0, Math.min(1, ((point.x - a[0]) * dx + (point.y - a[1]) * dy) / lengthSquared));

  return Math.hypot(point.x - (a[0] + t * dx), point.y - (a[1] + t * dy));
}

function distanceToBounds(bounds: Bounds, point: Point): number
{
  const dx = Math.max(bounds.x - point.x, 0, point.x - (bounds.x + bounds.w));
  const dy = Math.max(bounds.y - point.y, 0, point.y - (bounds.y + bounds.h));

  return Math.hypot(dx, dy);
}

function reach(item: Item, point: Point, zoom: number): { exact: boolean; hit: boolean; distance: number }
{
  const tolerance = minimumHitSize / 2 / zoom;

  if (item.type === 'path')
  {
    const points = item.points;
    const distance = points.length === 1 ? Math.hypot(point.x - points[0][0], point.y - points[0][1]) : Math.min(...points.slice(1).map((next, index) => distanceToSegment(point, points[index], next)));
    const half = item.width / 2;

    return { exact: distance <= half, hit: distance <= Math.max(half, tolerance), distance };
  }

  const bounds = itemBounds(item);
  const distance = distanceToBounds(bounds, point);
  const shortfall = Math.max(0, minimumHitSize / zoom - Math.min(bounds.w, bounds.h));
  return { exact: distance === 0, hit: distance <= shortfall / 2, distance };
}

export function hitItem(page: Page, point: Point, zoom: number): Item | undefined
{
  const topmostFirst = [...page.items].reverse();
  const loose = topmostFirst.filter((item) => item.type !== 'artboard').map((item) => ({ item, ...reach(item, point, zoom) })).filter((candidate) => candidate.exact || candidate.hit);
  const exact = loose.find((candidate) => candidate.exact);

  if (exact) return exact.item;

  const nearest = loose.reduce<(typeof loose)[number] | undefined>((best, candidate) => (!best || candidate.distance < best.distance ? candidate : best), undefined);

  if (nearest) return nearest.item;

  return topmostFirst.find((item) => item.type === 'artboard' && reach(item, point, zoom).exact);
}

export function cursorFor(state: CursorState): string
{
  if (state.resizeHandle) return handleCursor(state.resizeHandle);
  if (state.panGesture) return 'grabbing';
  if (state.panning) return 'grab';
  if (state.tool !== 'select') return 'crosshair';
  if (state.moving) return 'move';
  if (!state.hovered) return 'default';
  if (state.hovered.id === state.editingId) return 'text';
  if (state.hovered.id === state.selectedId && state.editable) return 'move';

  return 'default';
}
