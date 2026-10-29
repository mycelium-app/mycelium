<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { editor } from './lib/instance';

  let innerWidth = $state(1280);
  let innerHeight = $state(720);
  let root: HTMLDivElement;

  const artboard = $derived(editor.presenting === null ? undefined : editor.artboards[editor.presenting]);
  const scale = $derived(artboard ? Math.min(innerWidth / artboard.w, innerHeight / artboard.h) : 1);

  function onKeydown(event: KeyboardEvent)
  {
    if (event.key === 'Escape') editor.presenting = null;
    else if (['ArrowRight', 'ArrowDown', 'PageDown', ' '].includes(event.key)) editor.step(1);
    else if (['ArrowLeft', 'ArrowUp', 'PageUp'].includes(event.key)) editor.step(-1);
    else return;

    event.preventDefault();
  }

  onMount(() =>
  {
    void root.requestFullscreen?.().catch(() => undefined);
  });

  onDestroy(() =>
  {
    if (document.fullscreenElement)
    {
      void document.exitFullscreen();
    }
  });
</script>

<svelte:window bind:innerWidth bind:innerHeight onkeydown={onKeydown} />

<div class="present-stage" bind:this={root} role="dialog" aria-label="Presenting">
  {#if artboard}
    <iframe class="present-frame" sandbox="allow-scripts" srcdoc={artboard.html} title={artboard.name} style:width={`${artboard.w}px`} style:height={`${artboard.h}px`} style:transform={`scale(${scale})`}></iframe>
  {/if}
  <div class="present-controls">
    <button class="tool" type="button" aria-label="Previous artboard" disabled={editor.presenting === 0} onclick={() => editor.step(-1)}><Icon name="previous" /></button>
    <span>{artboard?.name}</span>
    <button class="tool" type="button" aria-label="Next artboard" disabled={editor.presenting === editor.artboards.length - 1} onclick={() => editor.step(1)}><Icon name="next" /></button>
    <button class="tool" type="button" aria-label="Exit present mode" onclick={() => (editor.presenting = null)}><Icon name="close" /></button>
  </div>
</div>
