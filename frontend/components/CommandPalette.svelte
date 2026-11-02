<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { filterCommands, paletteFocusIndex, type PaletteCommand } from '../lib/command-palette';
  import type { CatalogState } from '../lib/workbench';
  let {
    commands,
    catalog,
    oncommand,
    onclose,
    onreload,
  }: {
    commands: PaletteCommand[];
    catalog: CatalogState;
    oncommand: (command: PaletteCommand) => void;
    onclose: () => void;
    onreload: () => void;
  } = $props();
  let dialog: HTMLDialogElement, input: HTMLInputElement;
  let text = $state(''),
    focused = $state(-1);
  const matches = $derived(filterCommands(commands, text));
  const visible = $derived(matches.slice(0, 50));
  function choose(command: PaletteCommand): void {
    if (visible.includes(command) && !command.disabled) oncommand(command);
  }
  onMount(() => {
    dialog.showModal();
    input.focus();
  });
  onDestroy(() => {
    if (dialog.open) dialog.close();
  });
</script>

<dialog
  class="command-palette"
  bind:this={dialog}
  aria-labelledby="palette-title"
  oncancel={(event) => {
    event.preventDefault();
    onclose();
  }}
  onclick={(event) => {
    if (event.target === dialog) {
      const box = dialog.getBoundingClientRect();
      if (
        event.clientX < box.left ||
        event.clientX > box.right ||
        event.clientY < box.top ||
        event.clientY > box.bottom
      )
        onclose();
    }
  }}
  onkeydown={(event) => {
    if (event.key === 'Escape') {
      event.stopPropagation();
      return;
    }
    if (event.target === input && (event.key === 'Home' || event.key === 'End')) return;
    const next = paletteFocusIndex(event.key, focused, visible);
    if (next !== null) {
      event.preventDefault();
      focused = next;
      dialog.querySelectorAll<HTMLButtonElement>('[data-command]')[next]?.focus();
    }
  }}
>
  <header>
    <h2 id="palette-title">Commands</h2>
    <button onclick={onclose} aria-label="Close command palette">Esc</button>
  </header>
  <form
    onsubmit={(event) => {
      event.preventDefault();
      const command = visible[focused] ?? visible.find((command) => !command.disabled);
      if (command) choose(command);
    }}
  >
    <input
      bind:this={input}
      bind:value={text}
      oninput={() => (focused = -1)}
      type="search"
      aria-label="Find a command, saved view or favorite"
      placeholder="Search commands, views and favorite folders…"
      autocomplete="off"
      maxlength="200"
    />
  </form>
  {#if catalog.kind === 'loading'}<p role="status">Loading saved workspaces…</p>
  {:else if catalog.kind === 'failed'}<p role="alert">
      {catalog.message} <button onclick={onreload}>Reload</button>
    </p>{/if}
  <ul aria-label="Matching commands">
    {#each visible as command, i (command.id)}<li>
        <button
          data-command
          type="button"
          disabled={command.disabled}
          onfocus={() => (focused = i)}
          onclick={() => choose(command)}
        >
          <strong>{command.label}</strong><span title={command.detail}>{command.detail}</span>
        </button>
      </li>{/each}
  </ul>
  <footer role="status">
    {#if matches.length === 0}No matching commands.{:else}{visible.length} of {matches.length} commands{/if}<span
      >↑ ↓ Navigate · Enter Select</span
    >
  </footer>
</dialog>
