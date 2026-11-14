<script lang="ts">
  import { onMount } from 'svelte';
  import type { Destination } from '../lib/workbench';

  let {
    view,
    chart,
    waitingForScan,
    onclose,
  }: { view: Destination; chart: boolean; waitingForScan: boolean; onclose: () => void } = $props();
  let dialog: HTMLDialogElement;
  let heading: HTMLHeadingElement;

  const topics: Record<Destination, { title: string; hint: string }> = {
    overview: {
      title: 'Storage overview',
      hint: 'Use File types by size and Largest items to see where the scanned space goes. Open Applications to check storage by app.',
    },
    explorer: {
      title: 'Explorer',
      hint: 'Use Map, List or Grid to explore the current folder. Expand the chart for more room or open Items on map to browse it with the keyboard.',
    },
    large: {
      title: 'Large files',
      hint: 'Switch between files and folders, then use the minimum size filter to narrow the results.',
    },
    duplicates: {
      title: 'Duplicates',
      hint: 'Open a group to check its paths before adding copies to the review tray. Similar images may contain different data.',
    },
    cleanup: {
      title: 'Cleanup & operations',
      hint: 'Review selected files and their paths in the review tray. The preview lets you inspect the planned action before confirming it.',
    },
    developer: {
      title: 'Developer storage',
      hint: 'Open a project or cache group to see which scanned folders account for its size.',
    },
    history: {
      title: 'History',
      hint: 'Select a scan root and two compatible snapshots to compare changes. Playback steps through compatible snapshots for that root.',
    },
    settings: {
      title: 'Settings',
      hint: 'Set the theme in Appearance. Window contains desktop integration options. Use Scheduled scans to capture storage changes while Voldiq is running.',
    },
  };

  onMount(() => {
    dialog.showModal();
    heading.focus();
    return () => {
      if (dialog.open) dialog.close();
    };
  });
</script>

<dialog
  bind:this={dialog}
  class="workspace-help"
  aria-labelledby="workspace-help-title"
  oncancel={(event) => {
    event.preventDefault();
    onclose();
  }}
  onkeydown={(event) => {
    if (event.key === 'Escape' && !event.isComposing) {
      event.preventDefault();
      event.stopPropagation();
      onclose();
    }
  }}
>
  <header>
    <h2 id="workspace-help-title" bind:this={heading} tabindex="-1">Help & shortcuts</h2>
    <button class="quiet-action" aria-label="Close help" onclick={onclose}><kbd>Esc</kbd></button>
  </header>
  <section aria-labelledby="workspace-help-topic">
    <h3 id="workspace-help-topic">{waitingForScan ? 'First scan' : topics[view].title}</h3>
    <p>
      {waitingForScan
        ? 'Choose a folder, select a drive, or drop one folder into Voldiq. Scanning reads the selected location without removing files.'
        : topics[view].hint}
    </p>
  </section>
  <dl>
    <div>
      <dt><kbd>Ctrl</kbd> + <kbd>K</kbd></dt>
      <dd>Open commands</dd>
    </div>
    <div>
      <dt><kbd>F1</kbd></dt>
      <dd>Open help for this screen</dd>
    </div>
    {#if chart}
      <div>
        <dt><kbd>↑</kbd> <kbd>↓</kbd> <kbd>←</kbd> <kbd>→</kbd></dt>
        <dd>Pan focused chart</dd>
      </div>
      <div>
        <dt><kbd>+</kbd> <kbd>−</kbd> <kbd>0</kbd></dt>
        <dd>Zoom in, zoom out, fit chart</dd>
      </div>
      <div>
        <dt><kbd>Shift</kbd> + <kbd>F10</kbd></dt>
        <dd>Open chart actions</dd>
      </div>
    {/if}
  </dl>
</dialog>

<style>
  .workspace-help {
    width: min(480px, calc(100vw - 24px));
    max-height: 88dvh;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--canvas);
    color: var(--text);
  }
  .workspace-help::backdrop {
    background: #0005;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  h2 {
    font-size: 17px;
  }
  h3 {
    margin: 12px 0 4px;
    font-size: 14px;
  }
  p,
  dd {
    color: var(--secondary);
  }
  dl {
    margin: 12px 0 0;
  }
  dl > div {
    display: grid;
    grid-template-columns: minmax(92px, 1fr) 2fr;
    gap: 8px;
    align-items: baseline;
    padding-block: 8px;
    border-top: 1px solid var(--border);
  }
  dd {
    margin: 0;
  }
</style>
