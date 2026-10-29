<script lang="ts">
  import { onMount } from 'svelte';
  import App from './App.svelte';
  import CustomKind from './KindRoot.svelte';
  import { getArtifact } from './lib/api';
  import { parseRoute } from './lib/route';

  const route = parseRoute(location.pathname, location.search);
  let kind = $state<string | null>(route.mode === 'edit' ? null : 'design');

  onMount(async () =>
  {
    if (route.mode !== 'edit')
    {
      return;
    }

    try
    {
      kind = (await getArtifact(route.artifactId)).kind;
    }
    catch
    {
      kind = 'design';
    }
  });
</script>

{#if kind === null}
  <main class="message"><p role="status">Loading</p></main>
{:else if kind === 'design'}
  <App />
{:else if route.mode === 'edit'}
  <CustomKind artifactId={route.artifactId} />
{/if}
