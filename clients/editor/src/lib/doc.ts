export const designMediaType = 'application/vnd.mycelium.design+json';

export const blankArtboardHtml = '<!doctype html><html><head><meta charset="utf-8"></head><body style="margin:0;font-family:system-ui,sans-serif"></body></html>';

export type Artboard = { id: string; type: 'artboard'; name: string; x: number; y: number; w: number; h: number; html: string };
export type Shape = { id: string; type: 'rect' | 'ellipse'; x: number; y: number; w: number; h: number; fill: string; stroke: string | null; radius: number };
export type TextItem = { id: string; type: 'text'; x: number; y: number; w: number; text: string; size: number; color: string };
export type Note = { id: string; type: 'note'; x: number; y: number; w: number; h: number; text: string };
export type PathItem = { id: string; type: 'path'; points: [number, number][]; color: string; width: number };
export type Item = Artboard | Shape | TextItem | Note | PathItem;
export type ItemType = Item['type'];
export type Page = { id: string; name: string; items: Item[] };
export type DesignDoc = { pages: Page[] };
export type Bounds = { x: number; y: number; w: number; h: number };
export type Handle = 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w' | 'nw';
export type Alignment = 'left' | 'center' | 'right';
export type Stacking = 'front' | 'back';

export const minimumSize = 8;
export const duplicateOffset = 24;

export function emptyDesign(): DesignDoc
{
  return { pages: [{ id: 'p1', name: 'Page 1', items: [] }] };
}

export function parseDesign(text: string): DesignDoc
{
  const parsed = text.trim() === '' ? null : JSON.parse(text);

  if (!parsed || !Array.isArray(parsed.pages) || parsed.pages.length === 0)
  {
    return emptyDesign();
  }

  return {
    pages: parsed.pages.map((page: Page, index: number) => ({
      id: page.id ?? `p${index + 1}`,
      name: page.name ?? `Page ${index + 1}`,
      items: Array.isArray(page.items) ? page.items : [],
    })),
  };
}

export function serialiseDesign(doc: DesignDoc): string
{
  return JSON.stringify(doc);
}

export function designsEqual(a: DesignDoc, b: DesignDoc): boolean
{
  return a === b || JSON.stringify(a) === JSON.stringify(b);
}

export function allItems(doc: DesignDoc): Item[]
{
  return doc.pages.flatMap((page) => page.items);
}

export function findItem(doc: DesignDoc, id: string): Item | undefined
{
  return allItems(doc).find((item) => item.id === id);
}

export function pageOfItem(doc: DesignDoc, id: string): Page | undefined
{
  return doc.pages.find((page) => page.items.some((item) => item.id === id));
}

export function nextId(doc: DesignDoc, prefix: string): string
{
  const pattern = new RegExp(`^${prefix}(\\d+)$`);
  const used = [...allItems(doc).map((item) => item.id), ...doc.pages.map((page) => page.id)];
  const numbers = used.map((id) => pattern.exec(id)).filter((match) => match !== null).map((match) => Number(match[1]));

  return `${prefix}${Math.max(0, ...numbers) + 1}`;
}

export function idPrefix(type: ItemType): string
{
  const prefixes: Record<ItemType, string> = { artboard: 'a', rect: 's', ellipse: 's', text: 't', note: 'n', path: 'd' };

  return prefixes[type];
}

export function itemBounds(item: Item): Bounds
{
  if (item.type === 'path')
  {
    const xs = item.points.map((point) => point[0]);
    const ys = item.points.map((point) => point[1]);
    const x = Math.min(...xs);
    const y = Math.min(...ys);

    return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y };
  }

  if (item.type === 'text')
  {
    return { x: item.x, y: item.y, w: item.w, h: Math.ceil(item.size * 1.4 * Math.max(1, item.text.split('\n').length)) };
  }

  return { x: item.x, y: item.y, w: item.w, h: item.h };
}

export function updatePage(doc: DesignDoc, pageId: string, change: (page: Page) => Page): DesignDoc
{
  return { pages: doc.pages.map((page) => (page.id === pageId ? change(page) : page)) };
}

export function addPage(doc: DesignDoc): DesignDoc
{
  const id = nextId(doc, 'p');

  return { pages: [...doc.pages, { id, name: `Page ${doc.pages.length + 1}`, items: [] }] };
}

export function renamePage(doc: DesignDoc, pageId: string, name: string): DesignDoc
{
  return updatePage(doc, pageId, (page) => ({ ...page, name }));
}

export function addItem(doc: DesignDoc, pageId: string, item: Item): DesignDoc
{
  return updatePage(doc, pageId, (page) => ({ ...page, items: [...page.items, item] }));
}

export function updateItem(doc: DesignDoc, id: string, patch: Partial<Item>): DesignDoc
{
  return {
    pages: doc.pages.map((page) => ({
      ...page,
      items: page.items.map((item) => (item.id === id ? ({ ...item, ...patch } as Item) : item)),
    })),
  };
}

export function removeItem(doc: DesignDoc, id: string): DesignDoc
{
  return { pages: doc.pages.map((page) => ({ ...page, items: page.items.filter((item) => item.id !== id) })) };
}

export function translateItem(item: Item, dx: number, dy: number): Item
{
  if (item.type === 'path')
  {
    return { ...item, points: item.points.map(([x, y]) => [x + dx, y + dy] as [number, number]) };
  }

  return { ...item, x: item.x + dx, y: item.y + dy };
}

export function moveItem(doc: DesignDoc, id: string, dx: number, dy: number): DesignDoc
{
  return {
    pages: doc.pages.map((page) => ({
      ...page,
      items: page.items.map((item) => (item.id === id ? translateItem(item, dx, dy) : item)),
    })),
  };
}

export function resizeItem(doc: DesignDoc, id: string, handle: Handle, dx: number, dy: number): DesignDoc
{
  const item = findItem(doc, id);

  if (!item || item.type === 'path') return doc;

  const bounds = itemBounds(item);
  let { x, y, w, h } = bounds;

  if (handle.includes('e'))
  {
    w = Math.max(minimumSize, bounds.w + dx);
  }

  if (handle.includes('s'))
  {
    h = Math.max(minimumSize, bounds.h + dy);
  }

  if (handle.includes('w'))
  {
    w = Math.max(minimumSize, bounds.w - dx);
    x = bounds.x + bounds.w - w;
  }

  if (handle.includes('n'))
  {
    h = Math.max(minimumSize, bounds.h - dy);
    y = bounds.y + bounds.h - h;
  }

  if (item.type === 'text')
  {
    return updateItem(doc, id, { x, y, w });
  }

  return updateItem(doc, id, { x, y, w, h });
}

export function duplicateItem(doc: DesignDoc, id: string): { doc: DesignDoc; id: string | null }
{
  const item = findItem(doc, id);
  const page = pageOfItem(doc, id);

  if (!item || !page) return { doc, id: null };

  const copyId = nextId(doc, idPrefix(item.type));
  const moved = translateItem(item, duplicateOffset, duplicateOffset);
  const copy = { ...moved, id: copyId } as Item;
  const named = copy.type === 'artboard' ? { ...copy, name: `${copy.name} copy` } : copy;

  return { doc: addItem(doc, page.id, named), id: copyId };
}

export function stackItem(doc: DesignDoc, id: string, stacking: Stacking): DesignDoc
{
  const page = pageOfItem(doc, id);

  if (!page) return doc;

  const item = page.items.find((candidate) => candidate.id === id)!;
  const others = page.items.filter((candidate) => candidate.id !== id);

  return updatePage(doc, page.id, (current) => ({ ...current, items: stacking === 'front' ? [...others, item] : [item, ...others] }));
}

export function containingArtboard(doc: DesignDoc, id: string): Artboard | undefined
{
  const item = findItem(doc, id);
  const page = pageOfItem(doc, id);

  if (!item || !page || item.type === 'artboard') return undefined;

  const bounds = itemBounds(item);
  const centreX = bounds.x + bounds.w / 2;
  const centreY = bounds.y + bounds.h / 2;

  return page.items.find((candidate): candidate is Artboard => candidate.type === 'artboard' && contains(itemBounds(candidate), centreX, centreY));
}

export function alignItem(doc: DesignDoc, id: string, alignment: Alignment): DesignDoc
{
  const item = findItem(doc, id);
  const artboard = containingArtboard(doc, id);

  if (!item || !artboard || item.type === 'path') return doc;

  const bounds = itemBounds(item);
  const x = alignment === 'left' ? artboard.x : alignment === 'right' ? artboard.x + artboard.w - bounds.w : artboard.x + (artboard.w - bounds.w) / 2;

  return updateItem(doc, id, { x: Math.round(x) });
}

export function contains(bounds: Bounds, x: number, y: number): boolean
{
  return x >= bounds.x && x <= bounds.x + bounds.w && y >= bounds.y && y <= bounds.y + bounds.h;
}

export function hitTest(page: Page, x: number, y: number): Item | undefined
{
  const reversed = [...page.items].reverse();
  const loose = reversed.find((item) => item.type !== 'artboard' && contains(itemBounds(item), x, y));

  return loose ?? reversed.find((item) => item.type === 'artboard' && contains(itemBounds(item), x, y));
}

export function presentOrder(page: Page): Artboard[]
{
  const artboards = page.items.filter((item): item is Artboard => item.type === 'artboard');

  return [...artboards].sort((a, b) => a.y - b.y || a.x - b.x);
}

export function simplifyPoints(points: [number, number][], minimumDistance: number): [number, number][]
{
  const kept: [number, number][] = [];

  points.forEach((point, index) =>
  {
    const last = kept[kept.length - 1];
    const isEnd = index === points.length - 1;

    if (!last || isEnd || Math.hypot(point[0] - last[0], point[1] - last[1]) >= minimumDistance)
    {
      kept.push(point);
    }
  });

  return kept;
}

export function newItem(doc: DesignDoc, type: ItemType, bounds: Bounds): Item
{
  const id = nextId(doc, idPrefix(type));
  const x = Math.round(bounds.x);
  const y = Math.round(bounds.y);
  const w = Math.round(bounds.w);
  const h = Math.round(bounds.h);

  if (type === 'artboard')
  {
    const count = allItems(doc).filter((item) => item.type === 'artboard').length;

    return { id, type, name: `Artboard ${count + 1}`, x, y, w, h, html: blankArtboardHtml };
  }

  if (type === 'rect' || type === 'ellipse')
  {
    return { id, type, x, y, w, h, fill: '#7ccba0', stroke: null, radius: 0 };
  }

  if (type === 'text')
  {
    return { id, type, x, y, w, text: 'Text', size: 16, color: '#e3e6e4' };
  }

  if (type === 'note')
  {
    return { id, type, x, y, w, h, text: '' };
  }

  return { id, type: 'path', points: [[x, y]], color: '#e3e6e4', width: 2 };
}

export const defaultSizes: Record<Exclude<ItemType, 'path'>, { w: number; h: number }> = {
  artboard: { w: 800, h: 600 },
  rect: { w: 120, h: 80 },
  ellipse: { w: 120, h: 120 },
  text: { w: 200, h: 24 },
  note: { w: 240, h: 120 },
};
