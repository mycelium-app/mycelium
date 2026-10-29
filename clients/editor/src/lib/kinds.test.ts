import { describe, expect, it } from 'vitest';
import { addKeyframe, dashboardFileName, deleteSlide, docFromDom, duplicateSlide, formatClock, formatTime, frameAt, lastValue, lineStart, minutesAgo, moveSlide, newSlide, seriesSummary, sparkPath, stepSlide, styleOfTokens, timeLabel, type Deck, type DomNode, type Scene } from './kinds';

const deck: Deck = { slides: [{ id: 's1', title: 'One', body: ['a'], notes: '' }, { id: 's2', title: 'Two', body: [], notes: 'n' }] };

const text = (value: string): DomNode => ({ nodeName: '#text', nodeType: 3, textContent: value, childNodes: [] });
const element = (name: string, children: DomNode[], attributes: Record<string, string> = {}): DomNode => ({ nodeName: name.toUpperCase(), nodeType: 1, textContent: null, childNodes: children, getAttribute: (key) => attributes[key] ?? null });

describe('slides', () =>
{
  it('adds a blank slide after the current one', () =>
  {
    const next = newSlide(deck, 0);

    expect(next.slides.map((slide) => slide.title)).toEqual(['One', '', 'Two']);
    expect(new Set(next.slides.map((slide) => slide.id)).size).toBe(3);
  });

  it('duplicates a slide with its own body', () =>
  {
    const next = duplicateSlide(deck, 0);

    next.slides[1].body.push('b');

    expect(next.slides.map((slide) => slide.title)).toEqual(['One', 'One', 'Two']);
    expect(deck.slides[0].body).toEqual(['a']);
  });

  it('moves a slide and stops at the ends', () =>
  {
    expect(moveSlide(deck, 0, 1).slides.map((slide) => slide.id)).toEqual(['s2', 's1']);
    expect(moveSlide(deck, 0, -1)).toBe(deck);
  });

  it('keeps the last slide', () =>
  {
    expect(deleteSlide(deck, 0).slides.map((slide) => slide.id)).toEqual(['s2']);
    expect(deleteSlide({ slides: [deck.slides[0]] }, 0).slides).toHaveLength(1);
  });

  it('stays inside the deck when stepping', () =>
  {
    expect(stepSlide(0, -1, 6)).toBe(0);
    expect(stepSlide(5, 1, 6)).toBe(5);
    expect(stepSlide(2, 1, 6)).toBe(3);
  });

  it('shows the elapsed time as minutes and seconds', () =>
  {
    expect(formatClock(372)).toBe('06:12');
  });
});

describe('motion', () =>
{
  const scene: Scene = { duration: 4000, width: 1280, height: 720, layers: [{ id: 'l1', name: 'Ring', shape: 'ellipse', keyframes: [{ t: 0, x: 0, opacity: 0 }, { t: 2000, x: 100, opacity: 1 }] }] };

  it('formats the playhead like the transport shows it', () =>
  {
    expect(timeLabel(1200, 4000)).toBe('0:01.20 / 0:04.00');
    expect(formatTime(0)).toBe('0:00.00');
  });

  it('interpolates between keyframes and holds the ends', () =>
  {
    expect(frameAt(scene.layers[0], 1000)).toMatchObject({ x: 50, opacity: 0.5 });
    expect(frameAt(scene.layers[0], 5000)).toMatchObject({ x: 100, opacity: 1 });
    expect(frameAt(scene.layers[0], -5)).toMatchObject({ x: 0, opacity: 0 });
  });

  it('adds a keyframe on the selected layer at the playhead with the value it has there', () =>
  {
    const next = addKeyframe(scene, 'l1', 1000);

    expect(next.layers[0].keyframes?.map((keyframe) => keyframe.t)).toEqual([0, 1000, 2000]);
    expect(next.layers[0].keyframes?.[1]).toMatchObject({ x: 50, opacity: 0.5 });
  });

  it('replaces a keyframe already at the playhead', () =>
  {
    expect(addKeyframe(scene, 'l1', 2000).layers[0].keyframes).toHaveLength(2);
  });
});

describe('dashboards', () =>
{
  const series: [number, number][] = [[1000, 58], [2000, 61], [3000, 60]];

  it('summarises a series for assistive technology', () =>
  {
    expect(seriesSummary('Disk used', [{ name: 'x', points: series }])).toBe('Disk used: from 58 to 60, lowest 58, highest 61');
    expect(seriesSummary('Disk used', [])).toBe('Disk used: no data');
  });

  it('reads the last value of the first series', () =>
  {
    expect(lastValue([{ name: 'x', points: series }])).toBe(60);
    expect(lastValue([])).toBeNull();
  });

  it('draws a line across the box and none for a single point', () =>
  {
    expect(sparkPath(series, 100, 10)).toBe('M0.0 10.0L50.0 0.0L100.0 3.3');
    expect(sparkPath([[1, 1]], 100, 10)).toBe('');
  });

  it('names the download after the dashboard', () =>
  {
    expect(dashboardFileName('Backup health')).toBe('backup-health.json');
    expect(dashboardFileName('???')).toBe('dashboard.json');
  });
});

describe('typst source', () =>
{
  it('finds where a line starts so an error link can put the caret there', () =>
  {
    expect(lineStart('ab\ncd\nef', 3)).toBe(6);
    expect(lineStart('ab\ncd\nef', 1)).toBe(0);
  });

  it('says how long ago the last good version was', () =>
  {
    expect(minutesAgo(0, 120000)).toBe('2 minutes ago');
    expect(minutesAgo(0, 1000)).toBe('less than a minute ago');
  });
});

describe('doc', () =>
{
  it('reads headings, marks, links and lists out of the edited page', () =>
  {
    const root = element('div', [
      element('h1', [text('Guide')]),
      element('p', [text('Welcome to '), element('b', [text('harbour')]), element('a', [text('docs')], { href: 'https://example.com' })]),
      element('ul', [element('li', [text('One')]), element('li', [element('i', [text('Two')])])]),
    ]);

    expect(docFromDom(root)).toEqual({
      blocks: [
        { type: 'heading1', spans: [{ text: 'Guide' }] },
        { type: 'paragraph', spans: [{ text: 'Welcome to ' }, { text: 'harbour', bold: true }, { text: 'docs', link: 'https://example.com' }] },
        { type: 'bullet', spans: [{ text: 'One' }] },
        { type: 'bullet', spans: [{ text: 'Two', italic: true }] },
      ],
    });
  });

  it('keeps one empty paragraph when the page is empty', () =>
  {
    expect(docFromDom(element('div', []))).toEqual({ blocks: [{ type: 'paragraph', spans: [] }] });
  });
});

describe('design system', () =>
{
  it('turns tokens into css variables', () =>
  {
    expect(styleOfTokens({ colors: [{ token: 'brand', value: '#11305A' }], spacing: [], radius: [{ token: 'radius', value: '6px' }], type: [{ token: 'body', size: '15px' }], components: [] })).toBe(':root{--brand:#11305A;--radius:6px;--body:15px;}');
  });
});
