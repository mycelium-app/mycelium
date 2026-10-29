<script lang="ts">
  import type { ArtifactComment, CommentAnchor } from './lib/api';
  import { findItem, itemBounds } from './lib/doc';
  import { editor } from './lib/instance';
  import { submitsComment } from './lib/keys';

  let draft = $state('');

  function focusOnMount(node: HTMLTextAreaElement)
  {
    node.focus();
  }

  function screenPosition(anchor: CommentAnchor): { left: number; top: number } | null
  {
    let x = anchor.x;
    let y = anchor.y;

    if (anchor.item)
    {
      const item = findItem(editor.doc, anchor.item);

      if (!item) return null;

      const bounds = itemBounds(item);

      x += bounds.x;
      y += bounds.y;
    }

    return { left: x * editor.viewport.zoom + editor.viewport.x, top: y * editor.viewport.zoom + editor.viewport.y };
  }

  const pins = $derived(
    editor.comments
      .filter((comment) => comment.anchor?.page === editor.page.id)
      .map((comment) => ({ comment, position: screenPosition(comment.anchor!) }))
      .filter((pin): pin is { comment: ArtifactComment; position: { left: number; top: number } } => pin.position !== null),
  );
  const pending = $derived(editor.pendingAnchor ? screenPosition(editor.pendingAnchor) : null);

  async function post(event: SubmitEvent)
  {
    event.preventDefault();
    await editor.addComment(draft);
    draft = '';
  }

  function submitOnEnter(event: KeyboardEvent)
  {
    if (!submitsComment(event)) return;

    event.preventDefault();
    (event.currentTarget as HTMLTextAreaElement).form?.requestSubmit();
  }
</script>

{#each pins as { comment, position } (comment.id)}
  <div class="pin-anchor" style:left={`${position.left}px`} style:top={`${position.top}px`}>
    <button class="pin" class:resolved={comment.resolved} type="button" aria-label={`${comment.resolved ? 'Resolved comment' : 'Comment'} by ${comment.author_name}`} aria-expanded={editor.openThread === comment.id} onclick={() => (editor.openThread = editor.openThread === comment.id ? null : comment.id)}>
      <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M4 5h16v11H9l-5 4z" /></svg>
    </button>
    {#if editor.openThread === comment.id}
      <div class="thread" role="dialog" aria-label={`Comment by ${comment.author_name}`}>
        <strong>{comment.author_name}</strong>
        <p>{comment.body}</p>
        {#if !editor.viewOnly}
          <button class="text-button" type="button" onclick={() => void editor.setResolved(comment.id, !comment.resolved)}>{comment.resolved ? 'Reopen' : 'Resolve'}</button>
        {/if}
      </div>
    {/if}
  </div>
{/each}

{#if pending}
  <form class="pin-anchor thread new-thread" style:left={`${pending.left}px`} style:top={`${pending.top}px`} onsubmit={post} aria-label="New comment">
    <label>
      <span class="visually-hidden">Comment</span>
      <textarea bind:value={draft} rows="3" onkeydown={submitOnEnter} use:focusOnMount placeholder="Add a comment"></textarea>
    </label>
    <div class="thread-actions">
      <button class="text-button primary" type="submit">Post comment</button>
      <button class="text-button" type="button" onclick={() => (editor.pendingAnchor = null)}>Cancel</button>
    </div>
  </form>
{/if}
