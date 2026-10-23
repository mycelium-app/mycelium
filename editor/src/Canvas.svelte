<script lang="ts">
  import CommentLayer from './CommentLayer.svelte';
  import FloatingToolbar from './FloatingToolbar.svelte';
  import ItemView from './ItemView.svelte';
  import { type Bounds, type Handle, type ItemType, findItem, itemBounds, simplifyPoints } from './lib/doc';
  import { editor } from './lib/instance';
  import { type Point, panBy, screenToWorld, wheelZoomFactor, zoomAt } from './lib/viewport';

  type Gesture =
    | { kind: 'pan'; last: Point }
    | { kind: 'move'; id: string; start: Point; origin: Point; moved: boolean }
    | { kind: 'resize'; id: string; handle: Handle; start: Point }
    | { kind: 'create'; type: Exclude<ItemType, 'path'>; start: Point }
    | { kind: 'draw' }
    | { kind: 'pinch'; distance: number; centre: Point };

  const handles: Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'];
  const createTools = ['artboard', 'rect', 'ellipse', 'note', 'text'];
  const interactiveSelector = 'button, input, textarea, select, .floating, .thread, .pin-anchor';
  const moveThreshold = 3;

  let canvas: HTMLDivElement;
  let gesture: Gesture | null = null;
  let spaceHeld = $state(false);
  let ghost = $state.raw<(Bounds & { type: string }) | null>(null);
  let stroke = $state.raw<[number, number][]>([]);
  const pointers = new Map<number, Point>();

  const selection = $derived(editor.selected && editor.editingId !== editor.selected.id ? itemBounds(editor.selected) : null);
  const resizable = $derived(editor.selected !== undefined && editor.selected.type !== 'path' && !editor.viewOnly);
  const panning = $derived(editor.tool === 'hand' || spaceHeld);

  function localPoint(event: { clientX: number; clientY: number }): Point
  {
    const rect = canvas.getBoundingClientRect();

    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  function worldPoint(event: { clientX: number; clientY: number }): Point
  {
    return screenToWorld(editor.viewport, localPoint(event));
  }

  function itemAt(target: EventTarget | null): string | null
  {
    const element = (target as Element | null)?.closest('[data-item]');

    return element?.getAttribute('data-item') ?? null;
  }

  function normalise(a: Point, b: Point): Bounds
  {
    return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y) };
  }

  function pinchState(): { distance: number; centre: Point }
  {
    const [first, second] = [...pointers.values()];

    return { distance: Math.hypot(first.x - second.x, first.y - second.y), centre: { x: (first.x + second.x) / 2, y: (first.y + second.y) / 2 } };
  }

  function placeComment(event: PointerEvent)
  {
    const world = worldPoint(event);
    const id = itemAt(event.target);
    const item = id ? findItem(editor.doc, id) : undefined;
    const origin = item ? itemBounds(item) : { x: 0, y: 0 };

    editor.openThread = null;
    editor.pendingAnchor = { page: editor.page.id, item: item?.id ?? null, x: Math.round(world.x - origin.x), y: Math.round(world.y - origin.y) };
  }

  function onPointerDown(event: PointerEvent)
  {
    if ((event.target as Element).closest(interactiveSelector)) return;

    pointers.set(event.pointerId, localPoint(event));

    if (pointers.size === 2)
    {
      gesture = { kind: 'pinch', ...pinchState() };
      return;
    }

    if (event.button === 1 || (event.button === 0 && panning))
    {
      gesture = { kind: 'pan', last: localPoint(event) };
      canvas.setPointerCapture(event.pointerId);
      event.preventDefault();
      return;
    }

    if (event.button !== 0) return;

    const world = worldPoint(event);

    if (editor.tool === 'select')
    {
      const id = itemAt(event.target);

      editor.select(id);

      if (id && !editor.viewOnly)
      {
        gesture = { kind: 'move', id, start: world, origin: localPoint(event), moved: false };
      }

      return;
    }

    if (editor.tool === 'comment')
    {
      placeComment(event);
      return;
    }

    if (editor.tool === 'pen')
    {
      gesture = { kind: 'draw' };
      stroke = [[Math.round(world.x), Math.round(world.y)]];
      canvas.setPointerCapture(event.pointerId);
      return;
    }

    if (createTools.includes(editor.tool))
    {
      gesture = { kind: 'create', type: editor.tool as Exclude<ItemType, 'path'>, start: world };
      ghost = { type: editor.tool, ...normalise(world, world) };
      canvas.setPointerCapture(event.pointerId);
    }
  }

  function onPointerMove(event: PointerEvent)
  {
    if (!gesture) return;

    if (gesture.kind === 'pinch')
    {
      pointers.set(event.pointerId, localPoint(event));

      if (pointers.size < 2) return;

      const next = pinchState();
      const panned = panBy(editor.viewport, next.centre.x - gesture.centre.x, next.centre.y - gesture.centre.y);

      editor.viewport = zoomAt(panned, next.centre, next.distance / gesture.distance);
      gesture = { kind: 'pinch', ...next };
      return;
    }

    const local = localPoint(event);
    const world = worldPoint(event);

    if (gesture.kind === 'pan')
    {
      editor.viewport = panBy(editor.viewport, local.x - gesture.last.x, local.y - gesture.last.y);
      gesture = { kind: 'pan', last: local };
    }
    else if (gesture.kind === 'move')
    {
      if (!gesture.moved && Math.hypot(local.x - gesture.origin.x, local.y - gesture.origin.y) < moveThreshold) return;

      if (!gesture.moved)
      {
        canvas.setPointerCapture(event.pointerId);
        gesture.moved = true;
      }

      editor.previewDraft(editor.moveBy(gesture.id, world.x - gesture.start.x, world.y - gesture.start.y));
    }
    else if (gesture.kind === 'resize')
    {
      editor.previewDraft(editor.resizeBy(gesture.id, gesture.handle, world.x - gesture.start.x, world.y - gesture.start.y));
    }
    else if (gesture.kind === 'create')
    {
      ghost = { type: gesture.type, ...normalise(gesture.start, world) };
    }
    else if (gesture.kind === 'draw')
    {
      stroke = [...stroke, [Math.round(world.x), Math.round(world.y)]];
    }
  }

  function onPointerUp(event: PointerEvent)
  {
    pointers.delete(event.pointerId);

    const finished = gesture;

    if (finished?.kind === 'pinch' && pointers.size > 0) return;

    gesture = null;

    if (!finished) return;

    if (finished.kind === 'move' && finished.moved)
    {
      editor.finishDraft();
    }
    else if (finished.kind === 'resize')
    {
      editor.finishDraft();
    }
    else if (finished.kind === 'create' && ghost)
    {
      const bounds = ghost;
      const point = finished.start;

      ghost = null;
      editor.setTool('select');

      if (bounds.w < 6 && bounds.h < 6)
      {
        editor.createAt(finished.type, point.x, point.y);
      }
      else
      {
        const id = editor.create(finished.type, bounds);

        if (finished.type === 'text' || finished.type === 'note')
        {
          editor.editingId = id;
        }
      }
    }
    else if (finished.kind === 'draw')
    {
      editor.createPath(simplifyPoints(stroke, 2));
      stroke = [];
    }
  }

  function startResize(event: PointerEvent, handle: Handle)
  {
    if (!editor.selectedId) return;

    event.stopPropagation();
    event.preventDefault();
    gesture = { kind: 'resize', id: editor.selectedId, handle, start: worldPoint(event) };
    canvas.setPointerCapture(event.pointerId);
  }

  function observeSize(node: HTMLDivElement)
  {
    let fitted = false;
    const observer = new ResizeObserver(() =>
    {
      editor.setCanvasSize(node.clientWidth, node.clientHeight);

      if (!fitted)
      {
        fitted = true;
        editor.fit();
      }
    });

    observer.observe(node);

    return { destroy: () => observer.disconnect() };
  }

  function captureWheel(node: HTMLDivElement)
  {
    const onWheel = (event: WheelEvent) =>
    {
      event.preventDefault();

      if (event.ctrlKey || event.metaKey)
      {
        editor.viewport = zoomAt(editor.viewport, localPoint(event), wheelZoomFactor(event.deltaY));
      }
      else
      {
        editor.viewport = panBy(editor.viewport, -event.deltaX, -event.deltaY);
      }
    };

    node.addEventListener('wheel', onWheel, { passive: false });

    return { destroy: () => node.removeEventListener('wheel', onWheel) };
  }

  function onKeyChange(event: KeyboardEvent)
  {
    if (event.code !== 'Space') return;

    const target = event.target as HTMLElement;
    const typing = target.closest('input, textarea, select, button, [contenteditable]') !== null;

    if (event.type === 'keyup')
    {
      spaceHeld = false;
    }
    else if (!typing)
    {
      spaceHeld = true;
      event.preventDefault();
    }
  }
</script>

<svelte:window onkeydown={onKeyChange} onkeyup={onKeyChange} />

<div
  class="canvas"
  class:panning
  class:crosshair={editor.tool !== 'select' && !panning}
  bind:this={canvas}
  use:observeSize
  use:captureWheel
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={onPointerUp}
  role="region"
  aria-label="Canvas"
>
  <div class="world" style:transform={`translate(${editor.viewport.x}px, ${editor.viewport.y}px) scale(${editor.viewport.zoom})`} style:--inverse={1 / editor.viewport.zoom}>
    {#each editor.page.items as item (item.id)}
      <ItemView {item} />
    {/each}
    {#if selection}
      <div class="selection" style:left={`${selection.x}px`} style:top={`${selection.y}px`} style:width={`${selection.w}px`} style:height={`${selection.h}px`}>
        {#if resizable}
          {#each handles as handle (handle)}
            <div class={`selection-handle handle-${handle}`} onpointerdown={(event) => startResize(event, handle)} role="presentation"></div>
          {/each}
        {/if}
      </div>
    {/if}
    {#if ghost}
      <div class={`ghost ghost-${ghost.type}`} style:left={`${ghost.x}px`} style:top={`${ghost.y}px`} style:width={`${ghost.w}px`} style:height={`${ghost.h}px`}></div>
    {/if}
    {#if stroke.length > 1}
      <svg class="stroke" width="1" height="1" aria-hidden="true">
        <polyline points={stroke.map((point) => point.join(',')).join(' ')} fill="none" stroke="var(--text)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    {/if}
  </div>
  <CommentLayer />
  <FloatingToolbar />
</div>
