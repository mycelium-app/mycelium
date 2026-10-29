<script lang="ts">
  import { onDestroy } from 'svelte';
  import KindShell from './KindShell.svelte';
  import type { Artifact } from './lib/api';
  import { debounced, failureText, saveText, type Saved } from './lib/kindApi';
  import { addKeyframe, frameAt, timeLabel, type Scene } from './lib/kinds';

  let { artifact, text }: { artifact: Artifact; text: string } = $props();

  let scene = $state.raw<Scene>(JSON.parse(text));
  let time = $state(0);
  let playing = $state(false);
  let selected = $state<string | null>(null);
  let saved = $state<Saved>('saved');
  let failure = $state('');
  let frameHandle = 0;
  let startedAt = 0;
  let startedFrom = 0;

  const writer = debounced(() => void write());
  const selectedId = $derived(selected ?? scene.layers[0]?.id ?? null);

  async function write(): Promise<void>
  {
    saved = 'saving';

    try
    {
      await saveText(artifact.id, JSON.stringify(scene));
      saved = 'saved';
    }
    catch (error)
    {
      saved = 'error';
      failure = failureText(error);
    }
  }

  function tick(now: number): void
  {
    time = Math.min(scene.duration, startedFrom + (now - startedAt));

    if (time >= scene.duration)
    {
      playing = false;
      return;
    }

    frameHandle = requestAnimationFrame(tick);
  }

  function toggle(): void
  {
    if (playing)
    {
      playing = false;
      cancelAnimationFrame(frameHandle);
      return;
    }

    startedFrom = time >= scene.duration ? 0 : time;
    startedAt = performance.now();
    playing = true;
    frameHandle = requestAnimationFrame(tick);
  }

  function scrub(value: number): void
  {
    playing = false;
    cancelAnimationFrame(frameHandle);
    time = value;
  }

  function add(): void
  {
    if (!selectedId)
    {
      return;
    }

    scene = addKeyframe(scene, selectedId, time);
    saved = 'unsaved';
    writer.run();
  }

  function position(layer: Scene['layers'][number]): string
  {
    const frame = frameAt(layer, time);

    return `left:${(frame.x / scene.width) * 100}%;top:${(frame.y / scene.height) * 100}%;width:${(frame.w / scene.width) * 100}%;height:${(frame.h / scene.height) * 100}%;opacity:${frame.opacity};transform:rotate(${frame.rotation}deg);background:${layer.shape === 'text' ? 'transparent' : (layer.fill ?? '#7fb2ec')};border-radius:${layer.shape === 'ellipse' ? '50%' : '0'}`;
  }

  onDestroy(() =>
  {
    cancelAnimationFrame(frameHandle);
    writer.flush();
  });
</script>

<KindShell title={artifact.title} kind="Motion" {saved}>
  {#snippet actions()}
    <button type="button" class="kind-button" onclick={add} disabled={!selectedId}>Add keyframe</button>
  {/snippet}
  {#if failure}
    <p role="alert" class="kind-error">{failure}</p>
  {/if}
  <div class="motion">
    <div class="stage" style={`aspect-ratio:${scene.width} / ${scene.height};background:${scene.background ?? '#101418'}`} role="img" aria-label={`Scene at ${timeLabel(time, scene.duration)}, ${scene.layers.length} layers`}>
      {#each scene.layers as layer (layer.id)}
        <div class="layer" style={position(layer)}>{layer.shape === 'text' ? (layer.text ?? layer.name) : ''}</div>
      {/each}
    </div>
    <div class="transport">
      <button type="button" class="kind-button" onclick={() => scrub(0)}>Back to the start</button>
      <button type="button" class="kind-button" aria-pressed={playing} onclick={toggle}>{playing ? 'Pause' : 'Play'}</button>
      <output>{timeLabel(time, scene.duration)}</output>
      <input type="range" min="0" max={scene.duration} step="10" value={time} aria-label="Playhead" aria-valuetext={timeLabel(time, scene.duration)} oninput={(event) => scrub(Number(event.currentTarget.value))} />
    </div>
    <div class="timeline" role="group" aria-label="Layers">
      {#each scene.layers as layer (layer.id)}
        <div class="timeline-row">
          <button type="button" class="kind-button" aria-pressed={selectedId === layer.id} onclick={() => (selected = layer.id)}>{layer.name}</button>
          <div class="track">
            <div class="playhead" style={`left:${(time / scene.duration) * 100}%`} aria-hidden="true"></div>
            {#each layer.keyframes ?? [] as keyframe (keyframe.t)}
              <button type="button" class="keyframe" style={`left:${(keyframe.t / scene.duration) * 100}%`} aria-label={`${layer.name} keyframe at ${timeLabel(keyframe.t, scene.duration).split(' / ')[0]}`} onclick={() => scrub(keyframe.t)}></button>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  </div>
</KindShell>
