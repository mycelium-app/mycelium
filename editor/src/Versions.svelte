<script lang="ts">
  import Icon from './Icon.svelte';
  import { editor } from './lib/instance';

  const formatter = new Intl.DateTimeFormat('en-GB', { dateStyle: 'medium', timeStyle: 'short' });
  const newest = $derived([...editor.versions].sort((a, b) => b.version - a.version));
  const current = $derived(editor.artifact?.latest_version);
</script>

<aside class="panel" aria-label="Versions">
  <header class="panel-head">
    <h2>Versions</h2>
    <button class="tool" type="button" aria-label="Close versions" onclick={() => (editor.versionsOpen = false)}><Icon name="close" /></button>
  </header>
  <ol class="versions">
    {#each newest as version (version.version)}
      <li class="version">
        <div class="version-title">
          <strong>Version {version.version}</strong>
          <span>{version.author === 'agent' ? 'Agent' : 'You'}</span>
        </div>
        <time datetime={version.created_at}>{formatter.format(new Date(version.created_at))}</time>
        {#if version.note}
          <p>{version.note}</p>
        {/if}
        {#if version.version === current}
          <span class="current">Current</span>
        {:else}
          <button class="text-button" type="button" aria-label={`Restore version ${version.version}`} onclick={() => void editor.restore(version.version)}>Restore</button>
        {/if}
      </li>
    {/each}
  </ol>
</aside>
