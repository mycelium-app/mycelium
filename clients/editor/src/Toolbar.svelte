<script lang="ts">
  import Icon from './Icon.svelte';
  import { editor } from './lib/instance';
  import type { Tool } from './lib/keys';

  type ToolButton = { tool: Tool; label: string; icon: string; key: string };

  const tools: ToolButton[] = [
    { tool: 'select', label: 'Select', icon: 'select', key: 'V' },
    { tool: 'hand', label: 'Hand', icon: 'hand', key: 'H' },
    { tool: 'text', label: 'Text', icon: 'text', key: 'T' },
    { tool: 'artboard', label: 'Frame', icon: 'artboard', key: 'F' },
    { tool: 'note', label: 'Sticky note', icon: 'note', key: 'N' },
    { tool: 'pen', label: 'Pen', icon: 'pen', key: 'P' },
  ];
  const shapes: ToolButton[] = [
    { tool: 'rect', label: 'Rectangle', icon: 'rect', key: 'R' },
    { tool: 'ellipse', label: 'Ellipse', icon: 'ellipse', key: 'O' },
  ];
  const viewTools = ['select', 'hand'];

  let shapesOpen = $state(false);

  const visibleTools = $derived(editor.viewOnly ? tools.filter((entry) => viewTools.includes(entry.tool)) : tools);
  const shapeActive = $derived(shapes.some((entry) => entry.tool === editor.tool));
  const shapeIcon = $derived(shapes.find((entry) => entry.tool === editor.tool)?.icon ?? 'shapes');

  function chooseShape(tool: Tool)
  {
    editor.setTool(tool);
    shapesOpen = false;
  }
</script>

<div class="toolbar" role="toolbar" aria-label="Tools">
  <label class="page-picker">
    <span class="visually-hidden">Page</span>
    <select value={editor.page.id} onchange={(event) => editor.selectPage(event.currentTarget.value)}>
      {#each editor.doc.pages as page (page.id)}
        <option value={page.id}>{page.name}</option>
      {/each}
    </select>
  </label>
  {#if !editor.viewOnly}
    <button class="tool" type="button" aria-label="Add page" title="Add page" onclick={() => editor.addNewPage()}><Icon name="plus" /></button>
  {/if}
  <span class="divider" role="separator"></span>
  {#each visibleTools as entry (entry.tool)}
    <button class="tool" type="button" aria-label={entry.label} aria-keyshortcuts={entry.key} aria-pressed={editor.tool === entry.tool} title={`${entry.label} (${entry.key})`} onclick={() => editor.setTool(entry.tool)}>
      <Icon name={entry.icon} />
    </button>
  {/each}
  {#if !editor.viewOnly}
    <span class="menu-anchor">
      <button class="tool" type="button" aria-label="Shapes" aria-haspopup="true" aria-expanded={shapesOpen} aria-pressed={shapeActive} title="Shapes" onclick={() => (shapesOpen = !shapesOpen)}>
        <Icon name={shapeIcon} />
      </button>
      {#if shapesOpen}
        <div class="menu" role="menu" aria-label="Shapes">
          {#each shapes as entry (entry.tool)}
            <button class="menu-item" type="button" role="menuitem" aria-keyshortcuts={entry.key} onclick={() => chooseShape(entry.tool)}>
              <Icon name={entry.icon} />
              <span>{entry.label}</span>
              <kbd>{entry.key}</kbd>
            </button>
          {/each}
        </div>
      {/if}
    </span>
    <button class="tool" type="button" aria-label="Comment" aria-keyshortcuts="C" aria-pressed={editor.tool === 'comment'} title="Comment (C)" onclick={() => editor.setTool('comment')}>
      <Icon name="comment" />
    </button>
  {/if}
</div>
