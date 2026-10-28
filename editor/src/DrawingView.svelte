<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import KindShell from './KindShell.svelte';
  import type { Artifact } from './lib/api';
  import { debounced, failureText, saveText, type Saved } from './lib/kindApi';

  let { artifact, text }: { artifact: Artifact; text: string } = $props();

  let host: HTMLDivElement;
  let saved = $state<Saved>('saved');
  let failure = $state('');
  let unmount: (() => void) | null = null;
  let latest: string | null = null;

  const writer = debounced(() => void write());

  async function write(): Promise<void>
  {
    if (latest === null)
    {
      return;
    }

    const content = latest;

    latest = null;
    saved = 'saving';

    try
    {
      await saveText(artifact.id, content);
      saved = latest === null ? 'saved' : 'unsaved';
    }
    catch (error)
    {
      saved = 'error';
      failure = failureText(error);
    }
  }

  onMount(async () =>
  {
    const [{ createElement }, { createRoot }, excalidraw] = await Promise.all([import('react'), import('react-dom/client'), import('@excalidraw/excalidraw'), import('@excalidraw/excalidraw/index.css')]);
    const initial = JSON.parse(text);
    const dark = document.documentElement.dataset.theme !== 'light';
    const root = createRoot(host);
    let first = true;

    root.render(
      createElement(excalidraw.Excalidraw, {
        initialData: { elements: initial.elements ?? [], appState: { ...(initial.appState ?? {}), collaborators: new Map() }, files: initial.files ?? {} },
        theme: dark ? 'dark' : 'light',
        onChange: (elements: readonly unknown[], appState: unknown, files: unknown) =>
        {
          if (first)
          {
            first = false;
            return;
          }

          latest = excalidraw.serializeAsJSON(elements as never, appState as never, files as never, 'local');
          saved = 'unsaved';
          writer.run();
        },
      }),
    );
    unmount = () => root.unmount();
  });

  onDestroy(() =>
  {
    writer.flush();
    unmount?.();
  });
</script>

<KindShell title={artifact.title} kind="Drawing" {saved}>
  <p id="drawing-description" class="visually-hidden">An Excalidraw canvas. Use the toolbar to pick a shape, then drag on the canvas to draw it. Changes save by themselves.</p>
  {#if failure}
    <p role="alert" class="kind-error">{failure}</p>
  {/if}
  <div class="drawing" bind:this={host} role="group" aria-label="Drawing canvas" aria-describedby="drawing-description"></div>
</KindShell>
