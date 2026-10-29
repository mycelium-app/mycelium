export type Slide = { id: string; title: string; body: string[]; notes: string };
export type Deck = { slides: Slide[] };

export type Keyframe = { t: number; x?: number; y?: number; w?: number; h?: number; opacity?: number; rotation?: number };
export type Layer = { id: string; name: string; shape: 'rect' | 'ellipse' | 'text'; text?: string; fill?: string; x?: number; y?: number; w?: number; h?: number; keyframes?: Keyframe[] };
export type Scene = { duration: number; width: number; height: number; background?: string; layers: Layer[] };
export type Frame = { x: number; y: number; w: number; h: number; opacity: number; rotation: number };

export type Series = { name: string; points: [number, number][] };
export type Panel = { id: number; type: string; title: string; gridPos?: { x: number; y: number; w: number; h: number }; datasource?: { type?: string; uid?: string }; targets?: { expr?: string }[]; panels?: Panel[] };
export type Dashboard = { title: string; time?: { from: string; to: string }; panels: Panel[] };

export type Token = { name?: string; token: string; value?: string; size?: string };
export type DesignSystem = { colors: Token[]; spacing: Token[]; radius: Token[]; type: Token[]; components: { name: string; html: string }[] };

export const timeRanges = [
  { label: 'Last hour', range: '1h' },
  { label: 'Last 24 hours', range: '24h' },
  { label: 'Last 7 days', range: '7d' },
  { label: 'Last 30 days', range: '30d' },
] as const;

const defaultFrame: Frame = { x: 0, y: 0, w: 100, h: 100, opacity: 1, rotation: 0 };

function nextId(prefix: string, taken: string[]): string
{
  let counter = taken.length + 1;

  while (taken.includes(`${prefix}${counter}`))
  {
    counter += 1;
  }

  return `${prefix}${counter}`;
}

export function insertSlide(deck: Deck, index: number, slide: Slide): Deck
{
  const slides = [...deck.slides];

  slides.splice(index, 0, slide);

  return { slides };
}

export function newSlide(deck: Deck, after: number): Deck
{
  const id = nextId('s', deck.slides.map((slide) => slide.id));

  return insertSlide(deck, after + 1, { id, title: '', body: [], notes: '' });
}

export function duplicateSlide(deck: Deck, index: number): Deck
{
  const id = nextId('s', deck.slides.map((slide) => slide.id));
  const source = deck.slides[index];

  return insertSlide(deck, index + 1, { ...source, id, body: [...source.body] });
}

export function moveSlide(deck: Deck, index: number, step: -1 | 1): Deck
{
  const target = index + step;

  if (target < 0 || target >= deck.slides.length)
  {
    return deck;
  }

  const slides = [...deck.slides];

  [slides[index], slides[target]] = [slides[target], slides[index]];

  return { slides };
}

export function deleteSlide(deck: Deck, index: number): Deck
{
  if (deck.slides.length === 1)
  {
    return deck;
  }

  return { slides: deck.slides.filter((_, position) => position !== index) };
}

export function updateSlide(deck: Deck, index: number, change: Partial<Slide>): Deck
{
  return { slides: deck.slides.map((slide, position) => (position === index ? { ...slide, ...change } : slide)) };
}

export function stepSlide(current: number, step: -1 | 1, count: number): number
{
  return Math.min(count - 1, Math.max(0, current + step));
}

export function formatClock(seconds: number): string
{
  const whole = Math.max(0, Math.floor(seconds));

  return `${String(Math.floor(whole / 60)).padStart(2, '0')}:${String(whole % 60).padStart(2, '0')}`;
}

export function formatTime(milliseconds: number): string
{
  const centiseconds = Math.round(Math.max(0, milliseconds) / 10);
  const seconds = Math.floor(centiseconds / 100);

  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}.${String(centiseconds % 100).padStart(2, '0')}`;
}

export function timeLabel(time: number, duration: number): string
{
  return `${formatTime(time)} / ${formatTime(duration)}`;
}

export function frameAt(layer: Layer, time: number): Frame
{
  const base: Frame = { ...defaultFrame, x: layer.x ?? 0, y: layer.y ?? 0, w: layer.w ?? 100, h: layer.h ?? 100 };
  const keyframes = [...(layer.keyframes ?? [])].sort((first, second) => first.t - second.t);

  if (keyframes.length === 0)
  {
    return base;
  }

  const fill = (keyframe: Keyframe): Frame => ({ ...base, ...Object.fromEntries(Object.entries(keyframe).filter(([name]) => name !== 't')) });
  const after = keyframes.findIndex((keyframe) => keyframe.t >= time);

  if (after === 0)
  {
    return fill(keyframes[0]);
  }

  if (after === -1)
  {
    return fill(keyframes[keyframes.length - 1]);
  }

  const from = fill(keyframes[after - 1]);
  const to = fill(keyframes[after]);
  const span = keyframes[after].t - keyframes[after - 1].t;
  const progress = span === 0 ? 1 : (time - keyframes[after - 1].t) / span;
  const mix = (first: number, second: number) => first + (second - first) * progress;

  return { x: mix(from.x, to.x), y: mix(from.y, to.y), w: mix(from.w, to.w), h: mix(from.h, to.h), opacity: mix(from.opacity, to.opacity), rotation: mix(from.rotation, to.rotation) };
}

export function addKeyframe(scene: Scene, layerId: string, time: number): Scene
{
  const rounded = Math.round(time);

  return {
    ...scene,
    layers: scene.layers.map((layer) =>
    {
      if (layer.id !== layerId)
      {
        return layer;
      }

      const frame = frameAt(layer, rounded);
      const others = (layer.keyframes ?? []).filter((keyframe) => keyframe.t !== rounded);

      return { ...layer, keyframes: [...others, { t: rounded, ...frame }].sort((first, second) => first.t - second.t) };
    }),
  };
}

export function flattenPanels(panels: Panel[]): Panel[]
{
  return panels.flatMap((panel) => (panel.type === 'row' ? flattenPanels(panel.panels ?? []) : [panel]));
}

export function panelKind(panel: Panel): 'stat' | 'timeseries' | 'table' | 'other'
{
  return panel.type === 'stat' || panel.type === 'timeseries' || panel.type === 'table' ? panel.type : 'other';
}

export function dataSourceName(dashboard: Dashboard): string
{
  const sources = flattenPanels(dashboard.panels).flatMap((panel) => (panel.datasource?.uid ? [`${panel.datasource.type ? panel.datasource.type[0].toUpperCase() + panel.datasource.type.slice(1) : 'Data source'} on ${panel.datasource.uid}`] : []));

  return sources[0] ?? 'No data source';
}

export function lastValue(series: Series[]): number | null
{
  const points = series[0]?.points ?? [];

  return points.length === 0 ? null : points[points.length - 1][1];
}

export function formatValue(value: number | null): string
{
  if (value === null)
  {
    return 'No data';
  }

  return Math.abs(value) >= 100 ? String(Math.round(value)) : String(Math.round(value * 100) / 100);
}

export function sparkPath(points: [number, number][], width: number, height: number): string
{
  if (points.length < 2)
  {
    return '';
  }

  const times = points.map((point) => point[0]);
  const values = points.map((point) => point[1]);
  const [first, last] = [Math.min(...times), Math.max(...times)];
  const [low, high] = [Math.min(...values), Math.max(...values)];

  return points
    .map(([time, value], index) =>
    {
      const x = last === first ? 0 : ((time - first) / (last - first)) * width;
      const y = high === low ? height / 2 : height - ((value - low) / (high - low)) * height;

      return `${index === 0 ? 'M' : 'L'}${x.toFixed(1)} ${y.toFixed(1)}`;
    })
    .join('');
}

export function seriesSummary(title: string, series: Series[]): string
{
  const points = series[0]?.points ?? [];

  if (points.length === 0)
  {
    return `${title}: no data`;
  }

  const values = points.map((point) => point[1]);

  return `${title}: from ${formatValue(values[0])} to ${formatValue(values[values.length - 1])}, lowest ${formatValue(Math.min(...values))}, highest ${formatValue(Math.max(...values))}`;
}

export function dashboardFileName(title: string): string
{
  const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '');

  return `${slug || 'dashboard'}.json`;
}

export function lineNumbers(source: string): number[]
{
  return Array.from({ length: source.split('\n').length }, (_, index) => index + 1);
}

export function lineStart(source: string, line: number): number
{
  return source.split('\n').slice(0, line - 1).reduce((total, text) => total + text.length + 1, 0);
}

export function minutesAgo(then: number, now: number): string
{
  const minutes = Math.max(0, Math.round((now - then) / 60000));

  return minutes === 0 ? 'less than a minute ago' : minutes === 1 ? '1 minute ago' : `${minutes} minutes ago`;
}

export type DocBlock = { type: 'paragraph' | 'heading1' | 'heading2' | 'heading3' | 'bullet' | 'numbered'; spans: { text: string; bold?: boolean; italic?: boolean; link?: string }[] };
export type DocContent = { blocks: DocBlock[] };

export type DomNode = { nodeName: string; nodeType: number; textContent: string | null; childNodes: ArrayLike<DomNode>; getAttribute?: (name: string) => string | null };

type Marks = { bold?: boolean; italic?: boolean; link?: string };

function spansOf(node: DomNode, marks: Marks, out: DocBlock['spans']): void
{
  if (node.nodeType === 3)
  {
    if (node.textContent)
    {
      out.push({ text: node.textContent, ...marks });
    }

    return;
  }

  const name = node.nodeName.toLowerCase();
  const next: Marks = { ...marks };

  if (name === 'b' || name === 'strong')
  {
    next.bold = true;
  }

  if (name === 'i' || name === 'em')
  {
    next.italic = true;
  }

  if (name === 'a' && node.getAttribute?.('href'))
  {
    next.link = node.getAttribute('href') ?? undefined;
  }

  Array.from(node.childNodes).forEach((child) => spansOf(child, next, out));
}

const blockNames: Record<string, DocBlock['type']> = { h1: 'heading1', h2: 'heading2', h3: 'heading3' };

export function docFromDom(root: DomNode): DocContent
{
  const blocks: DocBlock[] = [];

  Array.from(root.childNodes).forEach((node) =>
  {
    const name = node.nodeName.toLowerCase();

    if (name === 'ul' || name === 'ol')
    {
      Array.from(node.childNodes).forEach((item) =>
      {
        const spans: DocBlock['spans'] = [];

        spansOf(item, {}, spans);
        blocks.push({ type: name === 'ul' ? 'bullet' : 'numbered', spans });
      });

      return;
    }

    const spans: DocBlock['spans'] = [];

    spansOf(node, {}, spans);
    blocks.push({ type: blockNames[name] ?? 'paragraph', spans });
  });

  return { blocks: blocks.length === 0 ? [{ type: 'paragraph', spans: [] }] : blocks };
}

export function colourTokens(system: DesignSystem): { name: string; token: string; value: string }[]
{
  return system.colors.map((colour) => ({ name: colour.name ?? colour.token, token: colour.token, value: colour.value ?? '' }));
}

export function styleOfTokens(system: DesignSystem): string
{
  const tokens = [...system.colors, ...system.spacing, ...system.radius].map((entry) => `--${entry.token}:${entry.value};`).concat(system.type.map((entry) => `--${entry.token}:${entry.size};`));

  return `:root{${tokens.join('')}}`;
}
