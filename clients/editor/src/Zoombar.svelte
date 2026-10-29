<script lang="ts">
  import Icon from './Icon.svelte';
  import { editor } from './lib/instance';
  import { zoomStep } from './lib/viewport';

  const saveLabels = { saved: 'Saved', saving: 'Saving', unsaved: 'Unsaved changes', error: 'Save failed, retrying' };
</script>

<div class="zoombar">
  {#if editor.viewOnly}
    <span class="status">View only</span>
  {:else}
    <span class="status" role="status" aria-live="polite">{saveLabels[editor.saveState]}</span>
    <button class="tool" type="button" aria-label="Undo" title="Undo (Ctrl Z)" disabled={!editor.canUndo} onclick={() => editor.undo()}><Icon name="undo" /></button>
    <button class="tool" type="button" aria-label="Redo" title="Redo (Ctrl Shift Z)" disabled={!editor.canRedo} onclick={() => editor.redo()}><Icon name="redo" /></button>
    <span class="divider" role="separator"></span>
  {/if}
  <button class="tool" type="button" aria-label="Zoom out" aria-keyshortcuts="-" title="Zoom out (-)" onclick={() => editor.zoomBy(1 / zoomStep)}><Icon name="minus" /></button>
  <output class="zoom-level" aria-label="Zoom">{Math.round(editor.viewport.zoom * 100)}%</output>
  <button class="tool" type="button" aria-label="Zoom in" aria-keyshortcuts="+" title="Zoom in (+)" onclick={() => editor.zoomBy(zoomStep)}><Icon name="plus" /></button>
  <button class="tool" type="button" aria-label="Fit to screen" aria-keyshortcuts="0" title="Fit (0)" onclick={() => editor.fit()}><Icon name="fit" /></button>
  <span class="divider" role="separator"></span>
  {#if !editor.viewOnly}
    <button class="tool" type="button" aria-label="Versions" aria-pressed={editor.versionsOpen} title="Versions" onclick={() => (editor.versionsOpen = !editor.versionsOpen)}><Icon name="history" /></button>
  {/if}
  <button class="present" type="button" disabled={editor.artboards.length === 0} onclick={() => editor.present()}><Icon name="present" /><span>Present</span></button>
</div>
