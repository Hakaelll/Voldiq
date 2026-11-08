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
  let text = $state(''),
    preferred = $state<string | null>(null);
  const matches = $derived(filterCommands(commands, text));
  const groups = $derived(groupCommands(matches.slice(0, 50)));
  const visible = $derived(groups.flatMap((group) => group.commands));
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
  <header>
    <h2 id="palette-title">Commands</h2>
    <button onclick={onclose} aria-label="Close command palette">Esc</button>
  </header>
  <div class="palette-search">
    <Icon name="search" />
    <input
      bind:this={input}
      bind:value={text}
      oninput={() => {
        preferred = null;
        results.scrollTop = 0;
      }}
      onkeydown={handleSearchKey}
      type="search"
      role="combobox"
      aria-label="Find a command, saved view or favorite"
      aria-autocomplete="list"
      aria-expanded="true"
      aria-controls="palette-options"
      aria-activedescendant={active ? optionId(active) : undefined}
      placeholder="Search commands, views and favorites…"
      autocomplete="off"
      maxlength="200"
    />
  </div>
  {#if catalog.kind === 'loading'}<p role="status">Loading saved workspaces…</p>
  {:else if catalog.kind === 'failed'}<p role="alert">
      {catalog.message} <button onclick={onreload}>Reload</button>
    </p>{/if}
  <div class="palette-results" bind:this={results}>
    <div id="palette-options" role="listbox" aria-label="Matching commands">
      {#each groups as { group, commands: options } (group)}
        <div role="group" aria-label={paletteGroups[group]}>
          <div class="palette-group-name" aria-hidden="true">{paletteGroups[group]}</div>
          {#each options as command (command.id)}
            <button
              type="button"
              role="option"
              id={optionId(command)}
              tabindex="-1"
              aria-selected={active === command}
              disabled={command.disabled}
              onpointerdown={(event) => {
                if (event.button === 0) event.preventDefault();
              }}
              onclick={() => choose(command)}
            >
              <Icon name={command.icon} size={17} />
              <strong title={command.label}>{command.label}</strong>
              {#if command.detail}<span title={command.detail}>{command.detail}</span>{/if}
            </button>
          {/each}
        </div>
      {/each}
    </div>
    {#if matches.length === 0}<p class="palette-empty">No matching commands.</p>{/if}
  </div>
  <footer>
    <span role="status">
      {#if matches.length > visible.length}{visible.length} of {matches.length} results
      {:else}{matches.length} {matches.length === 1 ? 'result' : 'results'}{/if}
    </span>
    <span><kbd>↑</kbd> <kbd>↓</kbd> Move <kbd>Enter</kbd> Open</span>
  </footer>
</dialog>
