<script lang="ts">
  import { onMount } from 'svelte';
  import KindShell from './KindShell.svelte';
  import { getArtifact, type Artifact } from './lib/api';
  import { followVersions, listArtifacts, loadText } from './lib/kindApi';
  import { colourTokens, styleOfTokens, type DesignSystem } from './lib/kinds';

  let { artifact, text }: { artifact: Artifact; text: string } = $props();

  let system = $state.raw<DesignSystem>(JSON.parse(text));
  let users = $state.raw<Artifact[]>([]);
  let known = artifact.latest_version;

  const sheet = $derived(styleOfTokens(system));

  onMount(() =>
  {
    void listArtifacts().then((all) => (users = all.filter((candidate) => candidate.kind === 'design' && candidate.design_system_id === artifact.id)));

    return followVersions(artifact.id, () => known, async () =>
    {
      const latest = await getArtifact(artifact.id);

      known = latest.latest_version;
      system = JSON.parse(await loadText(latest));
    });
  });
</script>

<KindShell title={artifact.title} kind="Design system" status="Changes are made by an agent in the chat">
  <div class="system">
    <section aria-labelledby="ds-colour">
      <h2 id="ds-colour">Colour</h2>
      <ul class="swatches">
        {#each colourTokens(system) as colour (colour.token)}
          <li><span class="swatch-chip" style={`background:${colour.value}`} aria-hidden="true"></span> <strong>{colour.name}</strong> <code>{colour.token}, {colour.value}</code></li>
        {/each}
      </ul>
    </section>
    <section aria-labelledby="ds-space">
      <h2 id="ds-space">Spacing and radius</h2>
      <ul>
        {#each [...system.spacing, ...system.radius] as token (token.token)}
          <li><code>{token.token}</code> {token.value}</li>
        {/each}
      </ul>
    </section>
    <section aria-labelledby="ds-type">
      <h2 id="ds-type">Type scale</h2>
      <ul>
        {#each system.type as entry (entry.token)}
          <li style={`font-size:${entry.size}`}><code>{entry.token}, {entry.size}</code> {entry.name ?? entry.token}</li>
        {/each}
      </ul>
    </section>
    <section aria-labelledby="ds-components">
      <h2 id="ds-components">Components</h2>
      {#each system.components as component (component.name)}
        <figure>
          <iframe title={component.name} sandbox="" srcdoc={`<style>${sheet}</style>${component.html}`}></iframe>
          <figcaption>{component.name}</figcaption>
        </figure>
      {/each}
    </section>
    <section aria-labelledby="ds-used">
      <h2 id="ds-used">Used by</h2>
      <ul>
        {#each users as design (design.id)}
          <li><a href={`/design/${design.id}`}>{design.title}</a></li>
        {/each}
      </ul>
    </section>
  </div>
</KindShell>
