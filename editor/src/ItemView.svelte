<script lang="ts">
  import Icon from './Icon.svelte';
  import type { Item } from './lib/doc';
  import { editor } from './lib/instance';

  let { item }: { item: Item } = $props();

  let renaming = $state(false);

  const selected = $derived(editor.selectedId === item.id);
  const entered = $derived(editor.enteredId === item.id);
  const editing = $derived(editor.editingId === item.id);
  const editable = $derived(!editor.viewOnly);

  function focusOnMount(node: HTMLTextAreaElement | HTMLInputElement)
  {
    node.focus();
    node.select();
  }

  function keepText(event: Event & { currentTarget: HTMLTextAreaElement })
  {
    editor.editingText = event.currentTarget.value;
  }

  function finishRename(event: Event & { currentTarget: HTMLInputElement })
  {
    const name = event.currentTarget.value.trim();

    renaming = false;

    if (item.type === 'artboard' && name !== '' && name !== item.name)
    {
      editor.patchItem(item.id, { name });
    }
  }

  function onTextKeydown(event: KeyboardEvent & { currentTarget: HTMLTextAreaElement })
  {
    if (event.key === 'Escape')
    {
      event.currentTarget.blur();
    }
  }

  function onRenameKeydown(event: KeyboardEvent & { currentTarget: HTMLInputElement })
  {
    if (event.key === 'Enter' || event.key === 'Escape')
    {
      event.currentTarget.blur();
    }
  }

  function startEditing()
  {
    if (editable && (item.type === 'text' || item.type === 'note'))
    {
      editor.editingId = item.id;
    }
  }

  function download()
  {
    if (item.type !== 'artboard') return;

    const url = URL.createObjectURL(new Blob([item.html], { type: 'text/html' }));
    const link = document.createElement('a');

    link.href = url;
    link.download = `${item.name}.html`;
    link.click();
    URL.revokeObjectURL(url);
  }

  function enter()
  {
    if (editable && item.type === 'artboard')
    {
      editor.enteredId = item.id;
    }
  }
</script>

{#if item.type === 'artboard'}
  <div class="artboard" class:selected data-item={item.id} style:left={`${item.x}px`} style:top={`${item.y}px`} style:width={`${item.w}px`} style:height={`${item.h}px`} ondblclick={enter} role="group" aria-label={item.name}>
    <div class="artboard-head" style:transform={`scale(${1 / editor.viewport.zoom})`}>
      {#if renaming}
        <input class="artboard-rename" aria-label="Artboard name" value={item.name} use:focusOnMount onblur={finishRename} onkeydown={onRenameKeydown} />
      {:else}
        <span class="artboard-name" ondblclick={(event) => { event.stopPropagation(); renaming = editable; }} role="presentation">{item.name}</span>
      {/if}
      <button class="mini" type="button" aria-label={`Zoom to ${item.name}`} title="Zoom to" onclick={() => editor.fit(item.id)}><Icon name="fit" /></button>
      <button class="mini" type="button" aria-label={`Present ${item.name}`} title="Present" onclick={() => editor.present(item.id)}><Icon name="expand" /></button>
      <button class="mini" type="button" aria-label={`Download ${item.name}`} title="Download" onclick={download}><Icon name="download" /></button>
    </div>
    <iframe class="artboard-frame" class:interactive={entered} sandbox="allow-scripts" srcdoc={item.html} title={item.name} tabindex={entered ? 0 : -1}></iframe>
  </div>
{:else if item.type === 'rect' || item.type === 'ellipse'}
  <div class="shape" class:selected data-item={item.id} style:left={`${item.x}px`} style:top={`${item.y}px`} style:width={`${item.w}px`} style:height={`${item.h}px`} style:background={item.fill} style:border={item.stroke ? `2px solid ${item.stroke}` : 'none'} style:border-radius={item.type === 'ellipse' ? '50%' : `${item.radius}px`}></div>
{:else if item.type === 'text'}
  <div class="text-item" class:selected data-item={item.id} style:left={`${item.x}px`} style:top={`${item.y}px`} style:width={`${item.w}px`} style:font-size={`${item.size}px`} style:color={item.color} ondblclick={startEditing} role="presentation">
    {#if editing}
      <textarea class="inline-edit" aria-label="Text" value={item.text} use:focusOnMount oninput={keepText} onblur={() => editor.finishEditing()} onkeydown={onTextKeydown} rows={Math.max(1, item.text.split('\n').length)}></textarea>
    {:else}
      {item.text}
    {/if}
  </div>
{:else if item.type === 'note'}
  <div class="note" class:selected data-item={item.id} style:left={`${item.x}px`} style:top={`${item.y}px`} style:width={`${item.w}px`} style:height={`${item.h}px`} ondblclick={startEditing} role="presentation">
    {#if editing}
      <textarea class="inline-edit" aria-label="Note" value={item.text} use:focusOnMount oninput={keepText} onblur={() => editor.finishEditing()} onkeydown={onTextKeydown}></textarea>
    {:else}
      {item.text}
    {/if}
  </div>
{:else}
  <svg class="path-item" class:selected data-item={item.id} width="1" height="1" role="img" aria-label="Drawing">
    <polyline points={item.points.map((point) => point.join(',')).join(' ')} fill="none" stroke={item.color} stroke-width={item.width} stroke-linecap="round" stroke-linejoin="round" />
  </svg>
{/if}
