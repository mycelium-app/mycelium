<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import Canvas from './Canvas.svelte';
  import Present from './Present.svelte';
  import Toolbar from './Toolbar.svelte';
  import Versions from './Versions.svelte';
  import Zoombar from './Zoombar.svelte';
  import { moveItem } from './lib/doc';
  import { editor } from './lib/instance';
  import { commandForKey } from './lib/keys';

  const nudges: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };

  function onKeydown(event: KeyboardEvent)
  {
    const target = event.target as HTMLElement;

    if (editor.presenting !== null || target.closest('input, textarea, select, [contenteditable]')) return;

    if (event.key === 'Escape')
    {
      if (editor.enteredId)
      {
        editor.enteredId = null;
      }
      else
      {
        editor.select(null);
      }

      return;
    }

    const nudge = nudges[event.key];

    if (nudge && editor.selectedId && !editor.viewOnly && !target.closest('button'))
    {
      const step = event.shiftKey ? 10 : 1;

      editor.edit(moveItem(editor.doc, editor.selectedId, nudge[0] * step, nudge[1] * step));
      event.preventDefault();
      return;
    }

    const command = commandForKey(event);

    if (command)
    {
      editor.run(command);
      event.preventDefault();
    }
  }

  onMount(() => void editor.load());
  onDestroy(() => editor.dispose());
</script>

<svelte:window onkeydown={onKeydown} />

{#if editor.failure}
  <main class="message">
    <h1>Design</h1>
    <p role="alert">{editor.failure}</p>
  </main>
{:else if !editor.loaded}
  <main class="message">
    <p role="status">Loading design</p>
  </main>
{:else}
  <main class="editor">
    <h1 class="doc-title">{editor.title}</h1>
    <Canvas />
    <div class="chrome">
      <Toolbar />
      <Zoombar />
    </div>
    {#if editor.versionsOpen && !editor.viewOnly}
      <Versions />
    {/if}
    {#if editor.notice}
      <p class="notice" role="status">{editor.notice}</p>
    {/if}
    {#if editor.enteredId}
      <p class="hint" role="status">Interacting with the artboard. Press Escape to go back to editing.</p>
    {/if}
  </main>
  {#if editor.presenting !== null}
    <Present />
  {/if}
{/if}
