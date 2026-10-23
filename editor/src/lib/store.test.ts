import { beforeAll, describe, expect, it, vi } from 'vitest';
import type { Editor } from './store.svelte';

let createEditor: () => Editor;

beforeAll(async () =>
{
  vi.stubGlobal('location', { pathname: '/design/a1', search: '' });
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
