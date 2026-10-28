<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import KindShell from './KindShell.svelte';
  import type { Artifact } from './lib/api';
  import { debounced, failureText, saveText, type Saved } from './lib/kindApi';
  import { docFromDom, type DocBlock, type DocContent } from './lib/kinds';

  let { artifact, text }: { artifact: Artifact; text: string } = $props();

  let page: HTMLDivElement;
  let saved = $state<Saved>('saved');
  let failure = $state('');
  let bold = $state(false);
  let italic = $state(false);
  let styleMenu = $state(false);
  let linkDialog = $state(false);
  let linkText = $state('');
  let linkAddress = $state('');
  let savedSelection: Range | null = null;

  const writer = debounced(() => void write());
  const styles = [
    { label: 'Normal text', tag: 'p' },
    { label: 'Heading 1', tag: 'h1' },
    { label: 'Heading 2', tag: 'h2' },
    { label: 'Heading 3', tag: 'h3' },
  ];
  let styleLabel = $state('Normal text');
  const tags: Record<string, string> = { heading1: 'h1', heading2: 'h2', heading3: 'h3', paragraph: 'p' };

  function render(content: DocContent): void
  {
    page.replaceChildren();
    let list: HTMLElement | null = null;

    for (const block of content.blocks)
    {
      const listTag = block.type === 'bullet' ? 'ul' : block.type === 'numbered' ? 'ol' : null;

      if (listTag)
      {
        if (!list || list.tagName.toLowerCase() !== listTag)
        {
          list = document.createElement(listTag);
          page.append(list);
        }

        const item = document.createElement('li');

        appendSpans(item, block);
        list.append(item);
        continue;
      }

      list = null;
      const element = document.createElement(tags[block.type] ?? 'p');

      appendSpans(element, block);

      if (block.spans.length === 0)
      {
        element.append(document.createElement('br'));
      }

      page.append(element);
    }
  }

  function appendSpans(parent: HTMLElement, block: DocBlock): void
  {
    for (const span of block.spans)
    {
      let node: Node = document.createTextNode(span.text);

      if (span.bold)
      {
        const wrapper = document.createElement('b');

        wrapper.append(node);
        node = wrapper;
      }

      if (span.italic)
      {
        const wrapper = document.createElement('i');

        wrapper.append(node);
        node = wrapper;
      }

      if (span.link && /^(https?:|mailto:|\/)/.test(span.link))
      {
        const anchor = document.createElement('a');

        anchor.href = span.link;
        anchor.append(node);
        node = anchor;
      }

      parent.append(node);
    }
  }

  async function write(): Promise<void>
  {
    saved = 'saving';

    try
    {
      await saveText(artifact.id, JSON.stringify(docFromDom(page)));
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
    writer.run();
  }

  function command(name: string, value?: string): void
  {
    page.focus();
    document.execCommand(name, false, value);
    marks();
    edited();
  }

  function marks(): void
  {
    bold = document.queryCommandState('bold');
    italic = document.queryCommandState('italic');
  }

  function chooseStyle(style: { label: string; tag: string }): void
  {
    styleMenu = false;
    styleLabel = style.label;
    command('formatBlock', style.tag);
  }

  function openLink(): void
  {
    const selection = window.getSelection();

    savedSelection = selection && selection.rangeCount > 0 ? selection.getRangeAt(0).cloneRange() : null;
    linkText = selection?.toString() ?? '';
    linkAddress = '';
    linkDialog = true;
  }

  function saveLink(): void
  {
    const address = linkAddress.trim();

    if (!/^(https?:|mailto:|\/)/.test(address))
    {
      return;
    }

    linkDialog = false;
    page.focus();
    const selection = window.getSelection();

    if (savedSelection && selection)
    {
      selection.removeAllRanges();
      selection.addRange(savedSelection);
    }

    if (selection?.isCollapsed && linkText)
    {
      const anchor = document.createElement('a');

      anchor.href = address;
      anchor.textContent = linkText;
      command('insertHTML', anchor.outerHTML);
      return;
    }

    command('createLink', address);
  }

  onMount(() => render(JSON.parse(text)));
  onDestroy(() => writer.flush());
</script>

<KindShell title={artifact.title} kind="Doc" {saved}>
  {#if failure}
    <p role="alert" class="kind-error">{failure}</p>
  {/if}
  <div class="doc-toolbar" role="toolbar" aria-label="Text formatting">
    <div class="menu-anchor">
      <button type="button" class="kind-button" aria-haspopup="menu" aria-expanded={styleMenu} onclick={() => (styleMenu = !styleMenu)}>{styleLabel}</button>
      {#if styleMenu}
        <div class="menu" role="menu" aria-label="Text style">
          {#each styles as style (style.tag)}
            <button type="button" class="menu-item" role="menuitemradio" aria-checked={style.label === styleLabel} onclick={() => chooseStyle(style)}>{style.label}</button>
          {/each}
        </div>
      {/if}
    </div>
    <button type="button" class="kind-button" aria-pressed={bold} onclick={() => command('bold')}>Bold</button>
    <button type="button" class="kind-button" aria-pressed={italic} onclick={() => command('italic')}>Italic</button>
    <button type="button" class="kind-button" onclick={openLink}>Link</button>
    <button type="button" class="kind-button" onclick={() => command('insertUnorderedList')}>Bulleted list</button>
    <button type="button" class="kind-button" onclick={() => command('insertOrderedList')}>Numbered list</button>
  </div>
  <div class="doc-page" bind:this={page} contenteditable="true" role="textbox" aria-multiline="true" aria-label="Document" tabindex="0" oninput={edited} onkeyup={marks} onmouseup={marks}></div>
  {#if linkDialog}
    <div class="kind-dialog" role="dialog" aria-modal="true" aria-labelledby="link-title">
      <h2 id="link-title">Link</h2>
      <label>Text <input bind:value={linkText} /></label>
      <label>Address <input bind:value={linkAddress} placeholder="https://" /></label>
      <div class="kind-dialog-actions">
        <button type="button" class="kind-button" onclick={() => (linkDialog = false)}>Cancel</button>
        <button type="button" class="kind-button kind-primary" onclick={saveLink}>Save</button>
      </div>
    </div>
  {/if}
</KindShell>
