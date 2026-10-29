<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import KindShell from './KindShell.svelte';
  import type { Artifact } from './lib/api';
  import { compileTypst, debounced, failureText, latestTypst, saveText, svgImage, type Saved, type TypstPreview } from './lib/kindApi';
  import { lineNumbers, lineStart, minutesAgo } from './lib/kinds';

  let { artifact, text }: { artifact: Artifact; text: string } = $props();

  let source = $state(text);
  let pages = $state<string[]>([]);
  let diagnostics = $state<TypstPreview['diagnostics']>([]);
  let goodAt = $state<number | null>(null);
  let compiling = $state(false);
  let saved = $state<Saved>('saved');
  let failure = $state('');
  let now = $state(Date.now());
  let area: HTMLTextAreaElement;
  let gutter: HTMLDivElement;

  const compiled = $derived(diagnostics.length === 0);
  const problem = $derived(diagnostics[0]);
  const status = $derived(compiling ? 'Compiling' : compiled ? 'Compiled' : 'Did not compile');
  const numbers = $derived(lineNumbers(source));

  const recompile = debounced(() => void compile(), 600);
  const writer = debounced(() => void write());

  async function compile(): Promise<void>
  {
    compiling = true;

    try
    {
      const preview = await compileTypst(source);

      diagnostics = preview.diagnostics;

      if (preview.compiled)
      {
        pages = preview.pages;
        goodAt = Date.now();
      }
    }
    catch (error)
    {
      failure = failureText(error);
    }
    finally
    {
      compiling = false;
    }
  }

  async function write(): Promise<void>
  {
    saved = 'saving';

    try
    {
      await saveText(artifact.id, source);
      saved = 'saved';
    }
    catch (error)
    {
      saved = 'error';
      failure = failureText(error);
    }
  }

  function edited(): void
  {
    saved = 'unsaved';
    recompile.run();
    writer.run();
  }

  function goToProblem(): void
  {
    if (!problem)
    {
      return;
    }

    area.focus();
    const start = lineStart(source, problem.line);

    area.setSelectionRange(start + Math.max(0, problem.column - 1), start + Math.max(0, problem.column - 1));
  }

  onMount(async () =>
  {
    try
    {
      const latest = await latestTypst(artifact.id);

      pages = latest.pages;
      diagnostics = latest.latest_compiled ? [] : latest.diagnostics;
      goodAt = latest.compiled_at ? Date.parse(latest.compiled_at) : null;
    }
    catch (error)
    {
      failure = failureText(error);
    }
  });

  const clock = setInterval(() => (now = Date.now()), 30000);

  onDestroy(() =>
  {
    clearInterval(clock);
    writer.flush();
  });
</script>

<KindShell title={artifact.title} kind="Typst" {saved} status={saved === 'saved' ? status : null}>
  {#snippet actions()}
    <a class="kind-button" href={`/api/v1/artifacts/${artifact.id}/pdf`} download>Download PDF</a>
  {/snippet}
  {#if failure}
    <p role="alert" class="kind-error">{failure}</p>
  {/if}
  {#if !compiled && problem}
    <div class="kind-summary" role="alert">
      <h2>The document did not compile</h2>
      <button type="button" class="kind-link" onclick={goToProblem}>Line {problem.line}: {problem.message}</button>
      <p>Typst: {problem.message}, line {problem.line}, column {problem.column}</p>
      {#if goodAt}
        <p>Showing the last version that compiled, from {minutesAgo(goodAt, now)}.</p>
      {/if}
    </div>
  {/if}
  <div class="typst">
    <div class="typst-source">
      <div class="typst-gutter" bind:this={gutter} aria-hidden="true">
        {#each numbers as number (number)}
          <div class:typst-bad={!compiled && problem?.line === number}>{number}</div>
        {/each}
      </div>
      <textarea bind:this={area} bind:value={source} oninput={edited} onscroll={() => (gutter.scrollTop = area.scrollTop)} aria-label="Typst source" spellcheck="false" aria-invalid={!compiled}></textarea>
    </div>
    <div class="typst-preview" class:typst-stale={!compiled} aria-label="Rendered document" role="region">
      {#each pages as page, index (index)}
        <img src={svgImage(page)} alt={`Page ${index + 1} of the rendered document`} />
      {/each}
    </div>
  </div>
</KindShell>
