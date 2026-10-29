<script lang="ts">
  import Icon from './Icon.svelte';
  import { itemBounds } from './lib/doc';
  import { editor } from './lib/instance';

  const item = $derived(editor.selected);
  const visible = $derived(item !== undefined && !editor.viewOnly && editor.editingId !== item.id && editor.draft === null);
  const bounds = $derived(item ? itemBounds(item) : null);
  const left = $derived(bounds ? (bounds.x + bounds.w / 2) * editor.viewport.zoom + editor.viewport.x : 0);
  const top = $derived(bounds ? Math.max(8, bounds.y * editor.viewport.zoom + editor.viewport.y - 52) : 0);
  const colourKey = $derived(item?.type === 'rect' || item?.type === 'ellipse' ? 'fill' : item?.type === 'text' || item?.type === 'path' ? 'color' : null);
  const colourValue = $derived(item && colourKey ? ((item as unknown as Record<string, string>)[colourKey] ?? '#000000') : '#000000');
  const alignable = $derived(item !== undefined && item.type !== 'artboard' && item.type !== 'path');

  let moreOpen = $state(false);

  $effect(() =>
  {
    editor.selectedId;
    moreOpen = false;
  });

  function setColour(event: Event & { currentTarget: HTMLInputElement })
  {
    if (colourKey)
    {
      editor.patchSelected({ [colourKey]: event.currentTarget.value });
    }
  }

  function setRadius(event: Event & { currentTarget: HTMLInputElement })
  {
    const radius = Math.max(0, Number(event.currentTarget.value) || 0);

    editor.patchSelected({ radius });
  }

  function choose(action: () => void)
  {
    moreOpen = false;
    action();
  }
</script>

{#if visible && item}
  <div class="floating" role="toolbar" aria-label="Selection" style:left={`${left}px`} style:top={`${top}px`}>
    {#if colourKey}
      <label class="swatch">
        <span class="visually-hidden">{colourKey === 'fill' ? 'Fill' : 'Colour'}</span>
        <input type="color" value={colourValue} onchange={setColour} />
      </label>
    {/if}
    {#if alignable}
      <button class="tool" type="button" aria-label="Align left" onclick={() => editor.alignSelected('left')}><Icon name="alignLeft" /></button>
      <button class="tool" type="button" aria-label="Align centre" onclick={() => editor.alignSelected('center')}><Icon name="alignCenter" /></button>
      <button class="tool" type="button" aria-label="Align right" onclick={() => editor.alignSelected('right')}><Icon name="alignRight" /></button>
    {/if}
    {#if item.type === 'rect'}
      <label class="radius">
        <span>Radius</span>
        <input type="number" min="0" value={item.radius} onchange={setRadius} />
      </label>
    {/if}
    <span class="menu-anchor">
      <button class="tool" type="button" aria-label="More" aria-haspopup="true" aria-expanded={moreOpen} onclick={() => (moreOpen = !moreOpen)}><Icon name="more" /></button>
      {#if moreOpen}
        <div class="menu" role="menu" aria-label="More">
          <button class="menu-item" type="button" role="menuitem" onclick={() => choose(() => editor.stackSelected('front'))}>Bring to front</button>
          <button class="menu-item" type="button" role="menuitem" onclick={() => choose(() => editor.stackSelected('back'))}>Send to back</button>
          <button class="menu-item" type="button" role="menuitem" onclick={() => choose(() => editor.duplicateSelected())}>Duplicate<kbd>Ctrl D</kbd></button>
          <button class="menu-item" type="button" role="menuitem" onclick={() => choose(() => editor.deleteSelected())}>Delete<kbd>Del</kbd></button>
        </div>
      {/if}
    </span>
  </div>
{/if}
