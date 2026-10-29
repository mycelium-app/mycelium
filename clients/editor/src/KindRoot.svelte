<script lang="ts">
  import { onMount } from 'svelte';
  import DashboardView from './DashboardView.svelte';
  import DesignSystemView from './DesignSystemView.svelte';
  import DocView from './DocView.svelte';
  import DrawingView from './DrawingView.svelte';
  import MotionView from './MotionView.svelte';
  import SlidesView from './SlidesView.svelte';
  import TypstView from './TypstView.svelte';
  import { ApiError, type Artifact } from './lib/api';
  import { loadKind } from './lib/kindApi';

  let { artifactId }: { artifactId: string } = $props();

  let loaded = $state.raw<{ artifact: Artifact; text: string } | null>(null);
  let failure = $state('');

  onMount(async () =>
  {
    try
    {
      loaded = await loadKind(artifactId);
    }
    catch (error)
    {
      failure = error instanceof ApiError && (error.status === 401 || error.status === 403) ? 'Sign in to Mycelium to open this artefact.' : 'The artefact could not be loaded. Reload the page to try again.';
    }
  });
</script>

{#if failure}
  <main class="message"><p role="alert">{failure}</p></main>
{:else if !loaded}
  <main class="message"><p role="status">Loading</p></main>
{:else if loaded.artifact.kind === 'drawing'}
  <DrawingView artifact={loaded.artifact} text={loaded.text} />
{:else if loaded.artifact.kind === 'typst'}
  <TypstView artifact={loaded.artifact} text={loaded.text} />
{:else if loaded.artifact.kind === 'doc'}
  <DocView artifact={loaded.artifact} text={loaded.text} />
{:else if loaded.artifact.kind === 'slides'}
  <SlidesView artifact={loaded.artifact} text={loaded.text} />
{:else if loaded.artifact.kind === 'dashboard'}
  <DashboardView artifact={loaded.artifact} text={loaded.text} />
{:else if loaded.artifact.kind === 'motion'}
  <MotionView artifact={loaded.artifact} text={loaded.text} />
{:else if loaded.artifact.kind === 'design_system'}
  <DesignSystemView artifact={loaded.artifact} text={loaded.text} />
{:else}
  <main class="message"><p role="alert">This kind of artefact opens in the app, not here.</p></main>
{/if}
