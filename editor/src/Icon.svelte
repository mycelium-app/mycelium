<script lang="ts">
  const paths: Record<string, string> = {
    select: 'M5 3l14 7-6 2-2 6z',
    hand: 'M8 12V6a1.5 1.5 0 013 0v5m0-6a1.5 1.5 0 013 0v6m0-4a1.5 1.5 0 013 0v7a6 6 0 01-6 6h-1a6 6 0 01-5-3l-3-5a1.5 1.5 0 012.5-1.5L8 14',
    text: 'M5 6V4h14v2M12 4v16M9 20h6',
    artboard: 'M7 3v18M17 3v18M3 7h18M3 17h18',
    note: 'M4 4h16v10l-6 6H4zM14 20v-6h6',
    pen: 'M4 20l1-5L16 4l4 4L9 19zM14 6l4 4',
    rect: 'M4 5h16v14H4z',
    ellipse: 'M12 4a8 7 0 100 14 8 7 0 000-14z',
    shapes: 'M4 12h9v8H4zM15 4a4.5 4.5 0 100 9 4.5 4.5 0 000-9z',
    comment: 'M4 5h16v11H9l-5 4z',
    present: 'M6 4l14 8-14 8z',
    expand: 'M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5',
    download: 'M12 4v11M7 11l5 5 5-5M5 20h14',
    plus: 'M12 5v14M5 12h14',
    minus: 'M5 12h14',
    fit: 'M3 8V3h5M21 8V3h-5M3 16v5h5M21 16v5h-5M9 9h6v6H9z',
    history: 'M4 12a8 8 0 108-8 8 8 0 00-6 3M4 4v4h4M12 8v5l3 2',
    undo: 'M9 14L4 9l5-5M4 9h10a6 6 0 010 12h-3',
    redo: 'M15 14l5-5-5-5M20 9H10a6 6 0 000 12h3',
    more: 'M5 12h.01M12 12h.01M19 12h.01',
    alignLeft: 'M4 4v16M8 8h12M8 16h7',
    alignCenter: 'M12 4v16M5 8h14M8 16h8',
    alignRight: 'M20 4v16M4 8h12M9 16h7',
    close: 'M6 6l12 12M18 6L6 18',
    previous: 'M15 5l-7 7 7 7',
    next: 'M9 5l7 7-7 7',
    check: 'M5 12l5 5 9-10',
  };

  let { name }: { name: string } = $props();
</script>

<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
  <path d={paths[name]} />
</svg>
