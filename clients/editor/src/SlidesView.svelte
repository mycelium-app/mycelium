<script lang="ts">
  import { onDestroy } from 'svelte';
  import KindShell from './KindShell.svelte';
  import type { Artifact } from './lib/api';
  import { debounced, failureText, saveText, type Saved } from './lib/kindApi';
  import { deleteSlide, duplicateSlide, formatClock, moveSlide, newSlide, stepSlide, updateSlide, type Deck } from './lib/kinds';

  let { artifact, text }: { artifact: Artifact; text: string } = $props();

  let deck = $state.raw<Deck>(JSON.parse(text));
  let current = $state(0);
  let saved = $state<Saved>('saved');
  let failure = $state('');
  let rowMenu = $state<number | null>(null);
  let presenting = $state(false);
  let elapsed = $state(0);
  let running = $state(true);
  let ticker: ReturnType<typeof setInterval> | undefined;

  const slide = $derived(deck.slides[current]);
  const next = $derived(deck.slides[current + 1]);
  const writer = debounced(() => void write());

  async function write(): Promise<void>
  {
    saved = 'saving';

    try
    {
      await saveText(artifact.id, JSON.stringify(deck));
      saved = 'saved';
    }
    catch (error)
    {
      saved = 'error';
      failure = failureText(error);
    }
  }

  function change(updated: Deck, show = current): void
  {
    deck = updated;
    current = Math.min(show, updated.slides.length - 1);
    saved = 'unsaved';
    rowMenu = null;
    writer.run();
  }

  function present(): void
  {
    presenting = true;
    elapsed = 0;
    running = true;
    ticker = setInterval(() =>
    {
      if (running)
      {
        elapsed += 1;
      }
    }, 1000);
  }

  function end(): void
  {
    presenting = false;
    clearInterval(ticker);
  }

  function onKeydown(event: KeyboardEvent): void
  {
    if (!presenting)
    {
      return;
    }

    if (event.key === 'Escape')
    {
      end();
    }
    else if (event.key === 'ArrowRight' || event.key === ' ' || event.key === 'ArrowDown')
    {
      current = stepSlide(current, 1, deck.slides.length);
    }
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp')
    {
      current = stepSlide(current, -1, deck.slides.length);
    }
    else
    {
      return;
    }

    event.preventDefault();
  }

  onDestroy(() =>
  {
    clearInterval(ticker);
    writer.flush();
  });
</script>

<svelte:window onkeydown={onKeydown} />

{#snippet face(shown: { title: string; body: string[] })}
  <h2>{shown.title}</h2>
  {#each shown.body as line, index (index)}
    <p>{line}</p>
  {/each}
{/snippet}

{#if presenting}
  <main class="presenter">
    <header class="kind-bar">
      <h1>{artifact.title}</h1>
      <span class="kind-tag">Presenter view</span>
      <span class="kind-status">Slide {current + 1} of {deck.slides.length}</span>
      <button type="button" class="kind-button" onclick={end}>End presentation</button>
    </header>
    <div class="presenter-grid">
      <section aria-label="Current slide">
        <div class="slide-face">{@render face(slide)}</div>
        <div class="kind-actions">
          <button type="button" class="kind-button" disabled={current === 0} onclick={() => (current = stepSlide(current, -1, deck.slides.length))}>Previous</button>
          <button type="button" class="kind-button" disabled={current === deck.slides.length - 1} onclick={() => (current = stepSlide(current, 1, deck.slides.length))}>Next</button>
        </div>
      </section>
      <aside>
        <p class="timer-label">Elapsed</p>
        <p class="timer" role="timer">{formatClock(elapsed)}</p>
        <div class="kind-actions">
          <button type="button" class="kind-button" aria-pressed={!running} onclick={() => (running = !running)}>{running ? 'Pause' : 'Resume'}</button>
          <button type="button" class="kind-button" onclick={() => (elapsed = 0)}>Reset</button>
        </div>
        {#if next}
          <p class="timer-label">Next: {next.title}</p>
          <div class="slide-face slide-small">{@render face(next)}</div>
        {:else}
          <p class="timer-label">Last slide</p>
        {/if}
        <h2 class="timer-label">Notes</h2>
        <p class="notes-large">{slide.notes}</p>
      </aside>
    </div>
  </main>
{:else}
  <KindShell title={artifact.title} kind="Slides" {saved}>
    {#snippet actions()}
      <button type="button" class="kind-button kind-primary" onclick={present}>Present</button>
    {/snippet}
    {#if failure}
      <p role="alert" class="kind-error">{failure}</p>
    {/if}
    <div class="slides">
      <nav class="slide-strip" aria-label="Slides">
        <ol>
          {#each deck.slides as row, index (row.id)}
            <li>
              <button type="button" class="slide-thumb" aria-current={index === current ? 'true' : undefined} onclick={() => (current = index)}>
                <span class="slide-number">{index + 1}</span>
                <span class="slide-name">{row.title || 'Untitled slide'}</span>
              </button>
              <div class="menu-anchor">
                <button type="button" class="kind-button" aria-haspopup="menu" aria-expanded={rowMenu === index} aria-label={`Slide ${index + 1} actions`} onclick={() => (rowMenu = rowMenu === index ? null : index)}>More</button>
                {#if rowMenu === index}
                  <div class="menu" role="menu">
                    <button type="button" class="menu-item" role="menuitem" onclick={() => change(moveSlide(deck, index, -1), Math.max(0, index - 1))}>Move up</button>
                    <button type="button" class="menu-item" role="menuitem" onclick={() => change(moveSlide(deck, index, 1), Math.min(deck.slides.length - 1, index + 1))}>Move down</button>
                    <button type="button" class="menu-item" role="menuitem" onclick={() => change(duplicateSlide(deck, index), index + 1)}>Duplicate</button>
                    <button type="button" class="menu-item" role="menuitem" onclick={() => change(deleteSlide(deck, index), Math.max(0, index - 1))}>Delete</button>
                  </div>
                {/if}
              </div>
            </li>
          {/each}
        </ol>
        <button type="button" class="kind-button" onclick={() => change(newSlide(deck, current), current + 1)}>New slide</button>
      </nav>
      <section class="slide-editor" aria-label={`Slide ${current + 1} of ${deck.slides.length}`}>
        <div class="slide-face">
          <label class="visually-hidden" for="slide-title">Title</label>
          <input id="slide-title" class="slide-title-input" value={slide.title} oninput={(event) => change(updateSlide(deck, current, { title: event.currentTarget.value }))} placeholder="Title" />
          <label class="visually-hidden" for="slide-body">Body, one line per paragraph</label>
          <textarea id="slide-body" class="slide-body-input" value={slide.body.join('\n')} oninput={(event) => change(updateSlide(deck, current, { body: event.currentTarget.value.split('\n') }))} placeholder="Body"></textarea>
        </div>
        <label class="notes-label" for="slide-notes">Speaker notes</label>
        <textarea id="slide-notes" class="notes-input" value={slide.notes} oninput={(event) => change(updateSlide(deck, current, { notes: event.currentTarget.value }))}></textarea>
      </section>
    </div>
  </KindShell>
{/if}
