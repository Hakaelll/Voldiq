<script lang="ts">
  import { onMount, tick } from 'svelte';
  import {
    filterCommands,
    groupCommands,
    paletteFocusIndex,
    paletteGroups,
    type PaletteCommand,
  } from '../lib/command-palette';
  import type { CatalogState } from '../lib/workbench';
  import Icon from './Icon.svelte';
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
  let dialog: HTMLDialogElement, input: HTMLInputElement, results: HTMLDivElement;
  let text = $state('');
  let preferred = $state<string | null>(null);
  const searching = $derived(text.trim().length > 0);
  const matches = $derived(filterCommands(commands, text));
  const visible = $derived(matches.slice(0, 50));
  const groups = $derived(searching ? [] : groupCommands(visible));
  const active = $derived(
    visible.find((command) => command.id === preferred && !command.disabled) ??
      visible.find((command) => !command.disabled),
  );

  function optionId(command: PaletteCommand): string {
    return `palette-option-${encodeURIComponent(command.id)}`;
  }

  function choose(command: PaletteCommand): void {
    if (visible.includes(command) && !command.disabled) oncommand(command);
  }

  function resetSelection(): void {
    preferred = null;
    results.scrollTop = 0;
  }

  async function handleSearchKey(event: KeyboardEvent): Promise<void> {
    if (event.isComposing || event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.key === 'Enter') {
      event.preventDefault();
      if (active) choose(active);
      return;
    }
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
    const next = paletteFocusIndex(event.key, active ? visible.indexOf(active) : -1, visible);
    if (next === null) return;
    event.preventDefault();
    const command = visible[next];
    preferred = command.id;
    await tick();
    if (preferred === command.id && dialog.open)
      document.getElementById(optionId(command))?.scrollIntoView({ block: 'nearest' });
  }

  onMount(() => {
    dialog.showModal();
    input.focus();
    return () => {
      if (dialog.open) dialog.close();
    };
  });
</script>

{#snippet commandRow(command: PaletteCommand)}
  <button
    type="button"
    role="option"
    id={optionId(command)}
    tabindex="-1"
    aria-selected={active === command}
    disabled={command.disabled}
    onmousedown={(event) => {
      if (event.button === 0) event.preventDefault();
    }}
    onpointermove={(event) => {
      if (event.pointerType === 'mouse' && !command.disabled) preferred = command.id;
    }}
    onclick={() => choose(command)}
  >
    <Icon name={command.icon} size={16} />
    <span class="palette-label" title={command.label}>{command.label}</span>
    {#if command.detail}
      <span class="palette-detail" title={command.detail}>{command.detail}</span>
    {/if}
  </button>
{/snippet}

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
    if (event.key === 'Escape' && !event.isComposing) {
      event.preventDefault();
      event.stopPropagation();
      onclose();
    }
  }}
>
  <h2 id="palette-title" class="sr-only">Commands</h2>
  <header class="palette-search">
    <div class="palette-field">
      <Icon name="search" size={20} />
      <input
        bind:this={input}
        bind:value={text}
        oninput={resetSelection}
        onkeydown={handleSearchKey}
        type="search"
        role="combobox"
        aria-label="Find a command, saved view or favorite"
        aria-autocomplete="list"
        aria-expanded={visible.length > 0}
        aria-controls="palette-options"
        aria-activedescendant={active ? optionId(active) : undefined}
        placeholder="Search commands, views and favorites"
        autocomplete="off"
        maxlength="200"
      />
    </div>
    <button
      type="button"
      class="quiet-action palette-close"
      onclick={onclose}
      aria-label="Close command palette"
    >
      <kbd aria-hidden="true">Esc</kbd>
    </button>
  </header>
  {#if catalog.kind === 'loading'}
    <div class="palette-catalog" role="status">Loading saved workspaces…</div>
  {:else if catalog.kind === 'failed'}
    <div class="palette-catalog">
      <span role="status" title={catalog.message}>Saved workspaces unavailable</span>
      <button
        type="button"
        class="quiet-action"
        onclick={() => {
          input.focus({ preventScroll: true });
          resetSelection();
          onreload();
        }}>Retry</button
      >
    </div>
  {/if}
  <div class="palette-results" bind:this={results}>
    <div
      id="palette-options"
      role="listbox"
      aria-label="Matching commands"
      hidden={!visible.length}
    >
      {#if searching}
        {#each visible as command (command.id)}
          {@render commandRow(command)}
        {/each}
      {:else}
        {#each groups as { group, commands: options } (group)}
          <div role="group" aria-label={paletteGroups[group]}>
            <div class="palette-group-name" aria-hidden="true">{paletteGroups[group]}</div>
            {#each options as command (command.id)}
              {@render commandRow(command)}
            {/each}
          </div>
        {/each}
      {/if}
    </div>
    {#if matches.length === 0}
      <div class="palette-empty">
        <strong>No matching commands</strong>
        <p>Try a page name, an action or a favorite folder.</p>
      </div>
    {/if}
  </div>
  <footer>
    <span role="status" class:sr-only={matches.length <= visible.length}>
      {#if matches.length > visible.length}Showing {visible.length} of {matches.length}. Keep typing
        to narrow results.
      {:else if searching}{matches.length} {matches.length === 1 ? 'result' : 'results'}{/if}
    </span>
    <div class="palette-shortcuts">
      <span><kbd>↑</kbd> <kbd>↓</kbd> Move</span>
      <span><kbd>Enter</kbd> Run</span>
    </div>
  </footer>
</dialog>
