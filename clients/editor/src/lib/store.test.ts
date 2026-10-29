import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import type { Editor } from './store.svelte';
import { emptyDesign } from './doc';

let createEditor: () => Editor;

beforeAll(async () =>
{
  vi.stubGlobal('location', { pathname: '/design/6f1c2a34-9b7d-4e55-8a10-3c2d1e0f4b66', search: '' });
  const { Editor } = await import('./store.svelte');
  createEditor = () => new Editor();
});

describe('editing text', () =>
{
  it('keeps the typed text when another item is selected without the field losing focus first', () =>
  {
    const editor = createEditor();
    const id = editor.createAt('text', 10, 10);

    editor.editingText = 'Hello';
    editor.select(null);

    expect(editor.doc.pages[0].items.find((item) => item.id === id)).toMatchObject({ text: 'Hello' });
  });

  it('keeps the typed text when another tool is picked', () =>
  {
    const editor = createEditor();
    const id = editor.createAt('note', 10, 10);

    editor.editingText = 'Remember this';
    editor.setTool('rect');

    expect(editor.doc.pages[0].items.find((item) => item.id === id)).toMatchObject({ text: 'Remember this' });
  });
});

const artifact = { id: '6f1c2a34-9b7d-4e55-8a10-3c2d1e0f4b66', kind: 'design', title: 'Design', slug: 'design-1', public: false, latest_version: 1, url: null, versions: [] };

afterEach(() =>
{
  vi.useRealTimers();
});

it('saves edits queued during a slow request as soon as that request finishes', async () =>
{
  vi.useFakeTimers();
  vi.stubGlobal('location', { pathname: '/design/6f1c2a34-9b7d-4e55-8a10-3c2d1e0f4b66', search: '' });
  let finishFirst: (response: Response) => void = () => {};
  const bodies: string[] = [];
  const fetch = vi.fn((_url: string, init: RequestInit) =>
  {
    bodies.push(String(init.body));

    if (bodies.length === 1)
    {
      return new Promise<Response>((resolve) => { finishFirst = resolve; });
    }

    return Promise.resolve(Response.json({ ...artifact, latest_version: 3 }));
  });
  vi.stubGlobal('fetch', fetch);
  const editor = createEditor();
  editor.artifact = artifact;
  const first = emptyDesign();
  first.pages[0].name = 'First edit';
  editor.edit(first);
  const saving = editor.save();
  const second = emptyDesign();
  second.pages[0].name = 'Second edit';
  editor.edit(second);
  await vi.advanceTimersByTimeAsync(1500);
  expect(bodies).toHaveLength(1);
  finishFirst(Response.json({ ...artifact, latest_version: 2 }));
  await saving;
  await vi.advanceTimersByTimeAsync(0);
  expect(bodies).toHaveLength(2);
  expect(JSON.parse(JSON.parse(bodies[1]).content).pages[0].name).toBe('Second edit');
  expect(editor.saveState).toBe('saved');
  editor.dispose();
});
