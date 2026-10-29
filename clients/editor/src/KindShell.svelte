<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { Saved } from './lib/kindApi';

  let { title, kind, saved = null, status = null, actions, children }: { title: string; kind: string; saved?: Saved | null; status?: string | null; actions?: Snippet; children: Snippet } = $props();

  const savedText: Record<Saved, string> = { saved: 'Saved', saving: 'Saving', unsaved: 'Unsaved changes', error: 'Not saved' };
</script>

<main class="kind">
  <header class="kind-bar">
    <h1>{title}</h1>
    <span class="kind-tag">{kind}</span>
    <span class="kind-status" role="status">{status ?? (saved ? savedText[saved] : '')}</span>
    <div class="kind-actions">{@render actions?.()}</div>
  </header>
  <div class="kind-body">{@render children()}</div>
</main>
