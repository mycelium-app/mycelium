<script lang="ts">
  import { onMount } from 'svelte';
  import KindShell from './KindShell.svelte';
  import { getArtifact, type Artifact } from './lib/api';
  import { failureText, followVersions, loadText, panelData } from './lib/kindApi';
  import { dashboardFileName, dataSourceName, flattenPanels, formatValue, lastValue, panelKind, seriesSummary, sparkPath, timeRanges, type Dashboard, type Panel, type Series } from './lib/kinds';

  let { artifact, text }: { artifact: Artifact; text: string } = $props();

  let json = $state(text);
  let known = artifact.latest_version;
  let range = $state('7d');
  let rangeMenu = $state(false);
  let jsonOpen = $state(false);
  let copied = $state(false);
  let data = $state<Record<number, { series: Series[]; failure: string }>>({});
  let views = $state<Record<number, 'chart' | 'table'>>({});

  const dashboard = $derived(JSON.parse(json) as Dashboard);
  const panels = $derived(flattenPanels(dashboard.panels));
  const rangeLabel = $derived(timeRanges.find((entry) => entry.range === range)?.label ?? range);

  async function refresh(): Promise<void>
  {
    for (const panel of panels)
    {
      try
      {
        data[panel.id] = { series: await panelData(artifact.id, panel.id, range), failure: '' };
      }
      catch (error)
      {
        data[panel.id] = { series: [], failure: failureText(error) };
      }
    }
  }

  function chooseRange(next: string): void
  {
    range = next;
    rangeMenu = false;
    void refresh();
  }

  async function copy(): Promise<void>
  {
    await navigator.clipboard.writeText(json);
    copied = true;
  }

  function download(): void
  {
    const link = document.createElement('a');

    link.href = URL.createObjectURL(new Blob([json], { type: 'application/json' }));
    link.download = dashboardFileName(dashboard.title);
    link.click();
    URL.revokeObjectURL(link.href);
  }

  function rows(series: Series[]): { time: string; value: string }[]
  {
    return (series[0]?.points ?? []).map(([time, value]) => ({ time: new Date(time).toLocaleDateString(undefined, { day: 'numeric', month: 'short' }), value: formatValue(value) }));
  }

  function viewOf(panel: Panel): 'chart' | 'table'
  {
    return views[panel.id] ?? (panelKind(panel) === 'table' ? 'table' : 'chart');
  }

  onMount(() =>
  {
    void refresh();

    return followVersions(artifact.id, () => known, async () =>
    {
      const latest = await getArtifact(artifact.id);

      known = latest.latest_version;
      json = await loadText(latest);
      void refresh();
    });
  });
</script>

<KindShell title={artifact.title} kind="Dashboard" status={dataSourceName(dashboard)}>
  {#snippet actions()}
    <div class="menu-anchor">
      <button type="button" class="kind-button" aria-haspopup="menu" aria-expanded={rangeMenu} onclick={() => (rangeMenu = !rangeMenu)}>{rangeLabel}</button>
      {#if rangeMenu}
        <div class="menu" role="menu" aria-label="Time range">
          {#each timeRanges as entry (entry.range)}
            <button type="button" class="menu-item" role="menuitemradio" aria-checked={entry.range === range} onclick={() => chooseRange(entry.range)}>{entry.label}</button>
          {/each}
        </div>
      {/if}
    </div>
    <button type="button" class="kind-button" onclick={() => void refresh()}>Refresh</button>
    <button type="button" class="kind-button" onclick={() => ((jsonOpen = true), (copied = false))}>JSON</button>
  {/snippet}
  <div class="dashboard">
    {#each panels as panel (panel.id)}
      {@const answer = data[panel.id]}
      {@const kind = panelKind(panel)}
      {@const wide = Math.min(24, Math.max(6, panel.gridPos?.w ?? 8))}
      <section class="panel" style={`grid-column: span ${Math.ceil(wide / 3)}`} aria-labelledby={`panel-${panel.id}`}>
        <header>
          <h2 id={`panel-${panel.id}`}>{panel.title}</h2>
          {#if kind === 'timeseries'}
            <div class="segmented" role="group" aria-label={`${panel.title} view`}>
              <button type="button" aria-pressed={viewOf(panel) === 'chart'} onclick={() => (views[panel.id] = 'chart')}>Chart</button>
              <button type="button" aria-pressed={viewOf(panel) === 'table'} onclick={() => (views[panel.id] = 'table')}>Table</button>
            </div>
          {/if}
        </header>
        {#if !answer}
          <p role="status">Loading</p>
        {:else if answer.failure}
          <p role="alert" class="kind-error">{answer.failure}</p>
        {:else if kind === 'stat'}
          <p class="stat">{formatValue(lastValue(answer.series))}</p>
        {:else if kind === 'timeseries' && viewOf(panel) === 'chart'}
          <svg class="chart" viewBox="0 0 300 100" preserveAspectRatio="none" role="img" aria-label={seriesSummary(panel.title, answer.series)}>
            <path d={sparkPath(answer.series[0]?.points ?? [], 300, 100)} fill="none" stroke="currentColor" stroke-width="2" vector-effect="non-scaling-stroke" />
          </svg>
        {:else if kind === 'other'}
          <p>This panel type ({panel.type}) is kept in the JSON but not drawn here.</p>
        {:else}
          <table>
            <thead><tr><th scope="col">Time</th><th scope="col">Value</th></tr></thead>
            <tbody>
              {#each rows(answer.series) as row, index (index)}
                <tr><td>{row.time}</td><td>{row.value}</td></tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </section>
    {/each}
  </div>
  {#if jsonOpen}
    <div class="kind-dialog kind-wide" role="dialog" aria-modal="true" aria-labelledby="json-title" tabindex="-1" onkeydown={(event) => event.key === 'Escape' && (jsonOpen = false)}>
      <h2 id="json-title">{artifact.title}: JSON</h2>
      <p>Grafana dashboard JSON. Import it into Grafana as it is, or ask an agent in the chat to change it.</p>
      <pre tabindex="0" aria-label="Dashboard JSON">{JSON.stringify(dashboard, null, 2)}</pre>
      <div class="kind-dialog-actions">
        <button type="button" class="kind-button" onclick={() => void copy()}>{copied ? 'Copied' : 'Copy'}</button>
        <button type="button" class="kind-button" onclick={download}>Download</button>
        <button type="button" class="kind-button kind-primary" onclick={() => (jsonOpen = false)}>Close</button>
      </div>
    </div>
  {/if}
</KindShell>
